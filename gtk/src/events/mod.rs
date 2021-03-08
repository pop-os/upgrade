pub mod background;

pub use self::background::BackgroundEvent;

use crate::{
    errors::UiError,
    get_dismiss_row, get_upgrade_row, notify, reboot,
    state::State,
    widgets::{dialogs::UpgradeDialog, permissions::PermissionDenied, Dismisser, Section},
};

use chrono::{TimeZone, Utc};
use gtk::prelude::*;

use pop_upgrade::{
    client::ReleaseInfo,
    daemon::{DaemonStatus, DISMISSED},
    release::{
        eol::{EolDate, EolStatus},
        UpgradeEvent,
    },
};

use std::{
    fs,
    path::Path,
    sync::{self, mpsc::SyncSender},
    thread,
};

/// Events received for the UI to handle.
#[derive(Debug)]
pub enum UiEvent {
    CancelledUpgrade,
    Completed(CompletedEvent),
    Dismissed(bool),
    Error(UiError),
    Initiated(InitiatedEvent),
    Progress(ProgressEvent),
    ReleaseUpgradeDialog,
    Shutdown,
    StatusChanged(DaemonStatus, DaemonStatus, Box<str>),
    Updated,
    Updating,
    UpgradeClicked,
    UpgradeEvent(UpgradeEvent),
    UpgradeNotificationClicked,
    WaitingOnLock,
}

#[derive(Debug)]
pub enum InitiatedEvent {
    Download(Box<str>),
    Recovery,
    Refresh,
    Scanning,
}

#[derive(Debug)]
pub enum CompletedEvent {
    Download,
    Recovery,
    Refresh,
    Scan(ScanEvent),
}

#[derive(Debug)]
pub enum ScanEvent {
    PermissionDenied,
    Found {
        is_lts:        bool,
        refresh:       bool,
        status_failed: bool,
        reboot_ready:  bool,
        upgrade_text:  Box<str>,
        upgrade:       Option<ReleaseInfo>,
    },
}

#[derive(Debug)]
pub enum ProgressEvent {
    Fetching(u64, u64),
    Recovery(u64, u64),
    Updates(u8),
}

#[repr(u8)]
pub enum Event {
    NotUpgrading = 0,
    Upgrading = 1,
    UpgradeReady = 2,
}

pub struct EventWidgets {
    pub button_sg:     gtk::SizeGroup,
    pub container:     gtk::Box,
    pub dismisser:     gtk::ListBoxRow,
    pub stack:         gtk::Stack,
    pub loading_label: gtk::Label,

    pub upgrade: Section,
}

impl EventWidgets {
    /// Sets the upgrade frame to display a permission denied widget.
    fn permission_denied(&self) {
        self.upgrade.frame.remove(&self.upgrade.list);
        self.upgrade.frame.add(PermissionDenied::new().as_ref());
        self.upgrade.frame.show_all();
    }
}

pub fn attach(gui_receiver: glib::Receiver<UiEvent>, widgets: EventWidgets, mut state: State) {
    gui_receiver.attach(None, move |event| {
        debug!("{:?}", event);
        match event {
            UiEvent::Progress(ProgressEvent::Fetching(progress, total)) => {
                let progress = state.calculate_fetching_progress(progress, total);
                widgets.upgrade.option.progress_exact(progress as u8).show_progress();
            }

            UiEvent::Progress(ProgressEvent::Recovery(progress, total)) => {
                widgets.upgrade.option.progress(progress, total).show_progress();
            }

            UiEvent::Progress(ProgressEvent::Updates(percent)) => {
                widgets.upgrade.option.progress_exact(percent / 4 + 25).show_progress();
            }

            UiEvent::Initiated(InitiatedEvent::Download(version)) => {
                widgets
                    .upgrade
                    .option
                    .label(&*["Downloading Pop!_OS ", &version].concat())
                    .reset_progress()
                    .show_progress();

                state.upgrading_to = version;
            }

            UiEvent::Initiated(InitiatedEvent::Refresh) => {
                get_upgrade_row(&widgets.upgrade.list).hide();
            }

            UiEvent::Initiated(InitiatedEvent::Scanning) => {
                widgets.loading_label.set_label("Checking for updates");
                widgets.stack.set_visible_child_name("loading");
                widgets.upgrade.option.reset_progress();
            }

            UiEvent::Initiated(InitiatedEvent::Recovery) => {
                widgets
                    .upgrade
                    .option
                    .label("Upgrading recovery partition")
                    .progress_exact(0)
                    .show_progress();
            }

            UiEvent::UpgradeEvent(UpgradeEvent::UpgradingPackages) => {
                widgets.upgrade.option.progress_exact(25);
            }

            UiEvent::UpgradeEvent(UpgradeEvent::UpdatingSourceLists) => {
                widgets.upgrade.option.progress_exact(50);
                state.fetching_release = true;
            }

            UiEvent::UpgradeEvent(_) => (),

            UiEvent::Completed(CompletedEvent::Download) => {
                download_complete(&mut state, &widgets);
            }

            UiEvent::Completed(CompletedEvent::Recovery) => {
                log::info!("successfully upgraded recovery partition");
            }

            UiEvent::Completed(CompletedEvent::Refresh) => reboot(),

            UiEvent::Completed(CompletedEvent::Scan(event)) => {
                widgets.stack.set_visible_child_name("updated");
                scan_event(&mut state, &widgets, event);
            }

            UiEvent::CancelledUpgrade => cancelled_upgrade(&mut state, &widgets),

            UiEvent::Updating => {
                widgets.loading_label.set_label("Updating the upgrade service");
                widgets.stack.set_visible_child_name("loading");
            }

            UiEvent::Updated => {
                widgets.stack.set_visible_child_name("updated");
            }

            UiEvent::UpgradeClicked => upgrade_clicked(&mut state, &widgets),

            UiEvent::UpgradeNotificationClicked => (state.callback_ready.borrow())(),

            UiEvent::ReleaseUpgradeDialog => release_upgrade_dialog(&mut state, &widgets),

            UiEvent::Dismissed(dismissed) => {
                log::info!("{} release", if dismissed { "dismissed" } else { "un-dismissed" });
                if let Some(dismisser) = state.dismisser.as_mut() {
                    dismisser.set_dismissed(dismissed);
                }
            }

            UiEvent::StatusChanged(from, to, why) => {
                log::warn!("status changed from {} to {}: {}", from, to, why);
                let _ = state.sender.send(BackgroundEvent::GetStatus(from));
            }

            UiEvent::Error(why) => error(&mut state, &widgets, &why),

            UiEvent::WaitingOnLock => (),

            UiEvent::Shutdown => return glib::Continue(false),
        }

        glib::Continue(true)
    });
}

/// On a cancelled upgrade, reset the widget to its pre-upgrade status.
fn cancelled_upgrade(state: &mut State, widgets: &EventWidgets) {
    (state.callback_event.borrow())(Event::NotUpgrading);

    state.upgrade_downloaded = false;
    widgets
        .upgrade
        .option
        .label(&*state.upgrade_label)
        .button_signal(Some(download_action(state.gui_sender.clone())))
        .reset_progress()
        .show_button();
}

/// Programs the refresh button
fn connect_refresh(state: &State, widgets: &EventWidgets) {
    // let sender = state.sender.clone();
    // let action = move || {
    //     let _ = sender.send(BackgroundEvent::RefreshOS);
    // };

    // widgets.refresh.option.button_signal(Some(("Refresh", action))).show();
}

/// Programs the upgrade button, and optionally enables the dismissal widget.
fn connect_upgrade(state: &mut State, widgets: &EventWidgets, is_lts: bool, reboot_ready: bool) {
    let notice = match EolDate::fetch() {
        Ok(eol) => {
            let (y, m, d) = eol.ymd;
            match eol.status() {
                EolStatus::Exceeded => Some(fomat!(
                    "Support for Pop!_OS " (eol.version) " has ended. "
                    "Security and application updates are no longer provided for Pop!_OS " (eol.version) ". "
                    "Upgrade to Pop!_OS " (eol.version.next_release()) " to keep your computer secure."
                )),
                EolStatus::Imminent => Some(fomat!(
                    "Support for Pop!_OS " (eol.version) " ends "
                    (Utc.ymd(y as i32, m, d).format("%B %-d, %Y")) ". "
                    "Upgrade for security and application updates."
                )),
                EolStatus::Ok => None,
            }
        }
        Err(why) => {
            log::error!("failed to fetch EOL date: {}", why);
            None
        }
    };

    let notice = notice.as_deref();

    widgets
        .upgrade
        .option
        .label(&state.upgrade_label)
        .sublabel(notice)
        .show_button()
        .button_signal({
            if let Some(info) = state.upgrade_version.as_ref() {
                state.upgrade_found = true;
                state.upgrading_from = info.current.clone();

                if is_lts {
                    get_dismiss_row(&widgets.upgrade.list).show();

                    set_dismissal_widget(
                        &widgets.button_sg,
                        state.sender.clone(),
                        state.dismisser.as_mut(),
                        widgets,
                        &info.next,
                    );
                }

                let gui_sender = state.gui_sender.clone();
                Some(if reboot_ready {
                    upgrade_action(gui_sender)
                } else {
                    download_action(gui_sender)
                })
            } else {
                None
            }
        });
}

/// Creates the download signal for the upgrade button.
fn download_action(sender: sync::Weak<glib::Sender<UiEvent>>) -> (&'static str, Box<dyn Fn()>) {
    let action: Box<dyn Fn()> = Box::new(move || {
        if let Some(sender) = sender.upgrade() {
            let _ = sender.send(UiEvent::UpgradeClicked);
        }
    });

    ("Download", action)
}

/// Notify that OS release updates have been downloaded, and are ready to commence.
fn download_complete(state: &mut State, widgets: &EventWidgets) {
    state.upgrade_downloaded = true;

    let sender = state.gui_sender.clone();
    let description = format!("Pop!_OS is ready to upgrade to {}", state.upgrading_to);
    thread::spawn(move || {
        notify::notify("distributor-logo", "Upgrade Ready", &description, || {
            if let Some(sender) = sender.upgrade() {
                let _ = sender.send(UiEvent::UpgradeNotificationClicked);
            }
        });
    });

    (state.callback_event.borrow())(Event::UpgradeReady);

    widgets
        .upgrade
        .option
        .show_button()
        .button_label("Upgrade")
        .label(&format!("Pop!_OS {} download complete", &*state.upgrading_to));
}

const GENERIC: &str = r#"Looks like we've encountered an issue! No worries, these are a list of files which may have been changed:

* /etc/apt/sources.list
* /etc/apt/sources.list.d/
* /etc/fstab

If you are a System76 customer, please run the System76 Driver tool to collect logs and contact support with the logs.

If you are seeing package manager issues, please run the following commands and send them to support in your support ticket:

sudo apt clean
sudo apt update -m
sudo dpkg --configure -a
sudo apt install -f
sudo apt dist-upgrade
sudo apt autoremove --purge"#;

/// Formats error messages for display on the console, and in the UI.
fn error(state: &mut State, widgets: &EventWidgets, why: &UiError) {
    let error_message = &mut format!("{}", why);
    why.iter_sources().for_each(|source| {
        error_message.push_str(": ");
        error_message.push_str(format!("{}", source).as_str());
    });

    (state.callback_error.borrow())(
        [GENERIC, format!("\n\nOriginating error cause:\n\n{}", error_message).as_str()]
            .concat()
            .as_str(),
    );

    log::error!("{}", error_message);

    if let UiError::Dismiss(dismissed, _) = why {
        if let Some(dismisser) = state.dismisser.as_mut() {
            dismisser.set_dismissed(!dismissed);
        }
    } else {
        (state.callback_event.borrow())(Event::NotUpgrading);
        reset(state, widgets)
    }
}

/// Checks if the release has been dismissed.
fn is_dismissed(next: &str) -> bool {
    if Path::new(DISMISSED).exists() {
        if let Ok(dismissed) = fs::read_to_string(DISMISSED) {
            if dismissed.as_str() == next {
                return true;
            }
        }
    }

    false
}

/// When the user selects to commence an upgrade, a dialog is shown to confirm.
fn release_upgrade_dialog(state: &mut State, widgets: &EventWidgets) {
    let dialog = UpgradeDialog::new(&state.upgrading_from, &state.upgrading_to);

    let answer = dialog.run();
    dialog.close();
    if gtk::ResponseType::Accept == answer {
        let _ = state.sender.send(BackgroundEvent::Finalize);
    } else {
        // Send upgrading event to prevent closing
        (state.callback_event.borrow())(Event::Upgrading);
        widgets.upgrade.option.label("Canceling upgrade");
        let _ = state.sender.send(BackgroundEvent::Reset);
    }
}

/// Resets widgets and state
fn reset(state: &mut State, widgets: &EventWidgets) {
    state.fetching_release = false;

    // if state.refresh_found {
    //     widgets.refresh.option.show_button();
    //     widgets.refresh.show();
    // }

    if state.upgrade_found {
        widgets.upgrade.option.show_button();
        get_upgrade_row(&widgets.upgrade.list).show();
    }
}

/// Creates a new dismisser widget, destroys any prior widget, and assigns it to the dismisser
/// frame.
fn set_dismissal_widget(
    button_sg: &gtk::SizeGroup,
    sender: SyncSender<BackgroundEvent>,
    dismisser: Option<&mut Dismisser>,
    widgets: &EventWidgets,
    next: &str,
) {
    let widget = Dismisser::new(next, move || {
        eprintln!("sending dismissal");
        let _ = sender.send(BackgroundEvent::DismissNotification(true));
    });

    widget.set_dismissed(is_dismissed(next));
    button_sg.add_widget(&widget.button);

    widgets.dismisser.foreach(|w| unsafe { w.destroy() });
    widgets.dismisser.add(widget.as_ref());
    widgets.dismisser.show_all();

    if let Some(dismisser) = dismisser {
        unsafe { dismisser.destroy() };
        *dismisser = widget;
    }
}

/// Handles release upgrade scan events.
fn scan_event(state: &mut State, widgets: &EventWidgets, event: ScanEvent) {
    match event {
        ScanEvent::PermissionDenied => widgets.permission_denied(),
        ScanEvent::Found {
            upgrade_text,
            upgrade,
            refresh,
            is_lts,
            status_failed,
            reboot_ready,
        } => {
            state.upgrade_label = upgrade_text;
            state.upgrade_version = upgrade;
            state.refresh_found = refresh;

            connect_upgrade(state, widgets, is_lts, reboot_ready);

            if refresh {
                connect_refresh(&state, widgets);
            }

            if status_failed {
                widgets.upgrade.option.hide_widgets();
            }

            widgets.container.show();
        }
    }
}

/// Creates the upgrade signal for the upgrade button.
fn upgrade_action(sender: sync::Weak<glib::Sender<UiEvent>>) -> (&'static str, Box<dyn Fn()>) {
    let action: Box<dyn Fn()> = Box::new(move || {
        if let Some(sender) = sender.upgrade() {
            let _ = sender.send(UiEvent::ReleaseUpgradeDialog);
        }
    });

    ("Upgrade", action)
}

/// Triggers on clicking the upgrade button
fn upgrade_clicked(state: &mut State, widgets: &EventWidgets) {
    if state.upgrade_downloaded {
        release_upgrade_dialog(state, widgets);
        return;
    }

    if let Some(info) = state.upgrade_version.clone() {
        (state.callback_event.borrow())(Event::Upgrading);
        widgets.upgrade.option.label("Preparing Upgrade").show_progress();
        // widgets.refresh.option.hide();

        if let Some(dismisser) = state.dismisser.take() {
            unsafe {
                dismisser.destroy();
            }
        }

        let _ = state.sender.send(BackgroundEvent::DownloadUpgrade(info));
    }
}
