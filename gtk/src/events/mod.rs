pub mod background;

pub use self::background::BackgroundEvent;
use crate::{
    errors::UiError,
    get_upgrade_row, notify, reboot,
    state::State,
    widgets::{
        dialogs::{RepositoryDialog, UpgradeDialog},
        permissions::PermissionDenied,
        Dismisser, UpgradeOption,
    },
};
use gtk::prelude::*;

use pop_upgrade::{
    client::{ReleaseInfo, RepoCompatError},
    daemon::{DaemonStatus, DISMISSED},
    release::UpgradeEvent,
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
    IncompatibleRepos(RepoCompatError),
    Initiated(InitiatedEvent),
    Progress(ProgressEvent),
    ReleaseUpgradeDialog,
    Shutdown,
    StatusChanged(DaemonStatus, DaemonStatus, Box<str>),
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
    pub container:       gtk::Box,
    pub dismisser_frame: gtk::Frame,
    pub options:         gtk::ListBox,
    pub option_upgrade:  UpgradeOption,
    pub option_refresh:  UpgradeOption,
    pub upgrade_frame:   gtk::Frame,
}

impl EventWidgets {
    /// Sets the upgrade frame to display a permission denied widget.
    fn permission_denied(&self) {
        self.upgrade_frame.remove(&self.options);
        self.upgrade_frame.add(PermissionDenied::new().as_ref());
        self.upgrade_frame.show_all();
        self.container.show();
    }
}

pub fn attach(gui_receiver: glib::Receiver<UiEvent>, widgets: EventWidgets, mut state: State) {
    gui_receiver.attach(None, move |event| {
        debug!("{:?}", event);
        match event {
            UiEvent::Progress(ProgressEvent::Fetching(progress, total)) => {
                let progress = state.calculate_fetching_progress(progress, total);
                widgets.option_upgrade.progress_exact(progress as u8).show_progress();
            }

            UiEvent::Progress(ProgressEvent::Recovery(progress, total)) => {
                widgets.option_upgrade.progress(progress, total).show_progress();
            }

            UiEvent::Progress(ProgressEvent::Updates(percent)) => {
                widgets.option_upgrade.progress_exact(percent / 4 + 25).show_progress();
            }

            UiEvent::Initiated(InitiatedEvent::Download(version)) => {
                // get_refresh_row(&widgets.options).hide();
                widgets
                    .option_upgrade
                    .label(&*["Downloading Pop!_OS ", &version].concat())
                    .reset_progress()
                    .show_progress();

                state.upgrading_to = version;
            }

            UiEvent::Initiated(InitiatedEvent::Refresh) => {
                get_upgrade_row(&widgets.options).hide();
            }

            UiEvent::Initiated(InitiatedEvent::Scanning) => {
                widgets.option_upgrade.reset_progress();
                widgets.container.hide();
            }

            UiEvent::Initiated(InitiatedEvent::Recovery) => {
                // get_refresh_row(&widgets.options).hide();
                widgets
                    .option_upgrade
                    .label("Upgrading recovery partition")
                    .progress_exact(0)
                    .show_progress();
            }

            UiEvent::UpgradeEvent(UpgradeEvent::UpgradingPackages) => {
                widgets.option_upgrade.progress_exact(25);
            }

            UiEvent::UpgradeEvent(UpgradeEvent::UpdatingSourceLists) => {
                widgets.option_upgrade.progress_exact(50);
                state.fetching_release = true;
            }

            UiEvent::UpgradeEvent(_) => (),

            UiEvent::Completed(CompletedEvent::Download) => {
                download_complete(&mut state, &widgets);
            }

            UiEvent::Completed(CompletedEvent::Recovery) => {
                info!("successfully upgraded recovery partition");
            }

            UiEvent::Completed(CompletedEvent::Refresh) => reboot(),

            UiEvent::Completed(CompletedEvent::Scan(event)) => {
                scan_event(&mut state, &widgets, event);
            }

            UiEvent::CancelledUpgrade => cancelled_upgrade(&mut state, &widgets),

            UiEvent::UpgradeClicked => upgrade_clicked(&mut state, &widgets),

            UiEvent::UpgradeNotificationClicked => (state.callback_ready.borrow())(),

            UiEvent::ReleaseUpgradeDialog => release_upgrade_dialog(&mut state, &widgets),

            UiEvent::Dismissed(dismissed) => {
                info!("{} release", if dismissed { "dismissed" } else { "un-dismissed" });
                if let Some(dismisser) = state.dismisser.as_mut() {
                    dismisser.set_dismissed(dismissed);
                }
            }

            UiEvent::IncompatibleRepos(repos) => incompatible_repos(&mut state, &widgets, repos),

            UiEvent::StatusChanged(from, to, why) => {
                warn!("status changed from {} to {}: {}", from, to, why);
                let _ = state.sender.send(BackgroundEvent::GetStatus(from));
            }

            UiEvent::Error(why) => error(&mut state, &widgets, why),

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
        .option_upgrade
        .label(&*state.upgrade_label)
        .button_signal(Some(download_action(state.gui_sender.clone())))
        .reset_progress()
        .show_button();
}

/// Programs the refresh button
fn connect_refresh(state: &State, widgets: &EventWidgets) {
    let sender = state.sender.clone();
    let action = move || {
        let _ = sender.send(BackgroundEvent::RefreshOS);
    };

    widgets.option_refresh.button_signal(Some(("Refresh", action))).show();
}

/// Programs the upgrade button, and optionally enables the dismissal widget.
fn connect_upgrade(state: &mut State, widgets: &EventWidgets, is_lts: bool, reboot_ready: bool) {
    widgets.option_upgrade.label(&state.upgrade_label).sublabel(None).show_button().button_signal(
        if let Some(info) = state.upgrade_version.as_ref() {
            state.upgrade_found = true;
            state.upgrading_from = info.current.clone();

            if is_lts {
                set_dismissal_widget(
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
        },
    );
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
        .option_upgrade
        .show_button()
        .button_label("Upgrade")
        .label(&format!("Pop!_OS {} download complete", &*state.upgrading_to));
}

/// Formats error messages for display on the console, and in the UI.
fn error(state: &mut State, widgets: &EventWidgets, why: UiError) {
    let error_message = &mut format!("{}", why);
    why.iter_sources().for_each(|source| {
        error_message.push_str(": ");
        error_message.push_str(format!("{}", source).as_str());
    });

    (state.callback_error.borrow())(error_message.as_str());

    error!("{}", error_message);

    if let UiError::Dismiss(dismissed, _) = why {
        if let Some(dismisser) = state.dismisser.as_mut() {
            dismisser.set_dismissed(!dismissed);
        }
    } else {
        (state.callback_event.borrow())(Event::NotUpgrading);
        reset(state, widgets)
    }
}

/// Runs the incompatible repository dialog, with a list of repositories
fn incompatible_repos(state: &mut State, widgets: &EventWidgets, repos: RepoCompatError) {
    let failures = repos
        .failure
        .into_iter()
        .map(|(repo, why)| {
            warn!("cannot upgrade {}: {}", repo, why);
            Box::from(repo)
        })
        .collect::<Vec<Box<str>>>();

    let dialog = RepositoryDialog::new(failures.iter());

    if gtk::ResponseType::Accept == dialog.run() {
        let _ = state
            .sender
            .send(BackgroundEvent::RepoModify(failures, dialog.answers().collect::<Vec<bool>>()));
    } else {
        (state.callback_event.borrow())(Event::NotUpgrading);
        reset(state, widgets)
    }

    dialog.destroy();
}

/// Checks if the release has been dismissed.
fn is_dismissed(next: &str) -> bool {
    if Path::new(DISMISSED).exists() {
        if let Some(dismissed) = fs::read_to_string(DISMISSED).ok() {
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
    dialog.destroy();
    if gtk::ResponseType::Accept == answer {
        let _ = state.sender.send(BackgroundEvent::Finalize);
    } else {
        // Send upgrading event to prevent closing
        (state.callback_event.borrow())(Event::Upgrading);
        widgets.option_upgrade.label("Canceling upgrade");
        let _ = state.sender.send(BackgroundEvent::Reset);
    }
}

/// Resets widgets and state
fn reset(state: &mut State, widgets: &EventWidgets) {
    state.fetching_release = false;

    if state.refresh_found {
        widgets.option_refresh.show_button();
        // get_refresh_row(&widgets.options).show();
    }

    if state.upgrade_found {
        widgets.option_upgrade.show_button();
        get_upgrade_row(&widgets.options).show();
    }
}

/// Creates a new dismisser widget, destroys any prior widget, and assigns it to the dismisser
/// frame.
fn set_dismissal_widget(
    sender: SyncSender<BackgroundEvent>,
    dismisser: Option<&mut Dismisser>,
    widgets: &EventWidgets,
    next: &str,
) {
    let widget = Dismisser::new(next, move |dismiss| {
        let _ = sender.send(BackgroundEvent::DismissNotification(dismiss));
    });

    widget.set_dismissed(is_dismissed(next));

    widgets.dismisser_frame.foreach(WidgetExt::destroy);
    widgets.dismisser_frame.add(widget.as_ref());
    widgets.dismisser_frame.show_all();

    if let Some(dismisser) = dismisser {
        dismisser.destroy();
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
                widgets.option_upgrade.hide_widgets();
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
        widgets.option_upgrade.label("Preparing Upgrade").show_progress();
        widgets.option_refresh.hide();

        if let Some(dismisser) = state.dismisser.take() {
            dismisser.destroy();
        }

        let _ = state.sender.send(BackgroundEvent::DownloadUpgrade(info));
    }
}
