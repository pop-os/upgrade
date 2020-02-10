pub mod background;

pub use self::background::{scan::ScanEvent, BackgroundEvent};

use crate::{
    errors::UiError,
    get_dismiss_row, get_upgrade_row, notify, reboot,
    state::State,
    widgets::{
        dialogs::{RefreshDialog, RepositoryDialog, UpgradeDialog},
        permissions::PermissionDenied,
        Dismisser, Section,
    },
    RECOVERY_PARTITION, REFRESH_OS,
};

use chrono::{TimeZone, Utc};
use gtk::prelude::*;

use pop_upgrade::{
    client::RepoCompatError,
    daemon::{DaemonStatus, DISMISSED},
    recovery::RecoveryEvent,
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
    Completed(CompletedEvent),
    Error(UiError),
    IncompatibleRepos(RepoCompatError),
    Initiated(InitiatedEvent),
    Progress(ProgressEvent),
    Recovery(OsRecoveryEvent),
    Shutdown,
    StatusChanged(DaemonStatus, DaemonStatus, Box<str>),
    Upgrade(OsUpgradeEvent),
    WaitingOnLock,
}

#[derive(Debug)]
pub enum OsUpgradeEvent {
    Cancelled,
    Dialog,
    Dismissed(bool),
    Event(UpgradeEvent),
    Notification,
    Upgrade,
}

#[derive(Debug)]
pub enum OsRecoveryEvent {
    Event(RecoveryEvent),
    Refresh,
    Reset,
    Update,
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
    pub button_sg: gtk::SizeGroup,
    pub container: gtk::Box,
    pub dismisser: gtk::ListBoxRow,
    pub recovery:  Section,
    pub upgrade:   Section,
}

impl EventWidgets {
    /// Sets the upgrade frame to display a permission denied widget.
    fn permission_denied(&self) {
        self.upgrade.frame.remove(&self.upgrade.list);
        self.upgrade.frame.add(PermissionDenied::new().as_ref());
        self.upgrade.frame.show_all();
        self.container.show();
    }
}

pub fn attach(gui_receiver: glib::Receiver<UiEvent>, widgets: EventWidgets, mut state: State) {
    gui_receiver.attach(None, move |event| {
        debug!("{:?}", event);
        match event {
            UiEvent::Progress(event) => match event {
                ProgressEvent::Fetching(progress, total) => {
                    let progress = state.calculate_fetching_progress(progress, total);
                    widgets.upgrade.options[0].progress_exact(progress as u8).show_progress();
                }

                ProgressEvent::Recovery(progress, total) => {
                    widgets.recovery.options[RECOVERY_PARTITION]
                        .label(&fomat!(
                            "Downloading the recovery partition update ("
                            (progress / 1024) " of " (total / 1024) " MiB)"
                        ))
                        .progress(progress, total)
                        .show_progress();
                }

                ProgressEvent::Updates(percent) => {
                    widgets.upgrade.options[0].progress_exact(percent / 4 + 25).show_progress();
                }
            },

            // Signals that a process in the background has begun.
            UiEvent::Initiated(event) => match event {
                InitiatedEvent::Download(version) => {
                    widgets.upgrade.options[0]
                        .label(&*["Downloading Pop!_OS ", &version].concat())
                        .reset_progress()
                        .show_progress();

                    state.upgrading_to = version;
                }

                InitiatedEvent::Refresh => {
                    get_upgrade_row(&widgets.upgrade.list).hide();
                }

                InitiatedEvent::Scanning => {
                    widgets.upgrade.options[0].reset_progress();
                    widgets.recovery.options[RECOVERY_PARTITION].reset_progress();
                    widgets.container.hide();
                }

                InitiatedEvent::Recovery => {
                    widgets.recovery.options[RECOVERY_PARTITION]
                        .label("Downloading the recovery partition update")
                        .progress_exact(0)
                        .show_progress();
                }
            },

            // Events pertaining to the upgrade section
            UiEvent::Upgrade(event) => match event {
                OsUpgradeEvent::Cancelled => cancelled_upgrade(&mut state, &widgets),

                OsUpgradeEvent::Dialog => release_upgrade_dialog(&mut state, &widgets),

                OsUpgradeEvent::Dismissed(dismissed) => {
                    info!("{} release", if dismissed { "dismissed" } else { "un-dismissed" });
                    if let Some(dismisser) = state.dismisser.as_mut() {
                        dismisser.set_dismissed(dismissed);
                    }
                }

                OsUpgradeEvent::Event(UpgradeEvent::UpgradingPackages) => {
                    widgets.upgrade.options[0].progress_exact(25);
                }

                OsUpgradeEvent::Event(UpgradeEvent::UpdatingSourceLists) => {
                    widgets.upgrade.options[0].progress_exact(50);
                    state.fetching_release = true;
                }

                OsUpgradeEvent::Event(_) => (),

                OsUpgradeEvent::Notification => (state.callback_ready.borrow())(),

                OsUpgradeEvent::Upgrade => upgrade_clicked(&mut state, &widgets),
            },

            // Events pertaining to the recovery section
            UiEvent::Recovery(event) => match event {
                OsRecoveryEvent::Event(event) => match event {
                    RecoveryEvent::Verifying => {
                        widgets.recovery.options[RECOVERY_PARTITION]
                            .label("Verifying the fetched recovery image")
                            .hide_widgets();
                    }
                    RecoveryEvent::Syncing => {
                        widgets.recovery.options[RECOVERY_PARTITION]
                            .label("Syncing recovery image to disk");
                    }
                    _ => (),
                },

                OsRecoveryEvent::Refresh => {
                    if gtk::ResponseType::Accept == RefreshDialog::new().run() {
                        let _ = state.sender.send(BackgroundEvent::RefreshOS);
                    } else {
                        widgets.recovery.options[REFRESH_OS].show_button();
                    }
                }

                OsRecoveryEvent::Reset => unimplemented!(),

                OsRecoveryEvent::Update => recovery::clicked(&mut state, &widgets),
            },

            UiEvent::Completed(event) => match event {
                CompletedEvent::Download => {
                    download_complete(&mut state, &widgets);
                }

                CompletedEvent::Recovery => {
                    widgets.upgrade.options[0].sensitive(true);
                    widgets.recovery.options[REFRESH_OS].sensitive(true);
                    widgets.recovery.options[RECOVERY_PARTITION]
                        .label("Recovery Partition")
                        .sublabel(Some(
                            "You have the most current version of the recovery partition",
                        ))
                        .hide_widgets();
                }

                CompletedEvent::Refresh => reboot(),

                CompletedEvent::Scan(event) => {
                    scan_event(&mut state, &widgets, event);
                }
            },

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
    widgets.upgrade.options[0]
        .label(&*state.upgrade_label)
        .button_signal(Some(download_action(state.gui_sender.clone())))
        .reset_progress()
        .show_button();
}

/// Programs the refresh button
fn connect_refresh(state: &State, widgets: &EventWidgets) {
    let action = enclose!((state.gui_sender => sender) move || {
        if let Some(sender) = sender.upgrade() {
            let _ = sender.send(UiEvent::Recovery(OsRecoveryEvent::Refresh));
        }
    });

    widgets.recovery.options[REFRESH_OS].button_signal(Some(("Refresh", action))).show();
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
            error!("failed to fetch EOL date: {}", why);
            None
        }
    };

    let notice = notice.as_ref().map(String::as_str);

    widgets.upgrade.options[0]
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
            let _ = sender.send(UiEvent::Upgrade(OsUpgradeEvent::Upgrade));
        }
    });

    ("Download", action)
}

/// Notify that OS release updates have been downloaded, and are ready to commence.
fn download_complete(state: &mut State, widgets: &EventWidgets) {
    state.upgrade_downloaded = true;

    let description = format!("Pop!_OS is ready to upgrade to {}", state.upgrading_to);
    thread::spawn(enclose!((state.gui_sender => sender) move || {
        notify::notify("distributor-logo", "Upgrade Ready", &description, || {
            if let Some(sender) = sender.upgrade() {
                let _ = sender.send(UiEvent::Upgrade(OsUpgradeEvent::Notification));
            }
        });
    }));

    (state.callback_event.borrow())(Event::UpgradeReady);

    widgets.upgrade.options[0]
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
    if gtk::ResponseType::Accept == dialog.run() {
        let _ = state.sender.send(BackgroundEvent::Finalize);
    } else {
        // Send upgrading event to prevent closing
        (state.callback_event.borrow())(Event::Upgrading);
        widgets.upgrade.options[0].label("Canceling upgrade");
        let _ = state.sender.send(BackgroundEvent::Reset);
    }
}

/// Resets widgets and state
fn reset(state: &mut State, widgets: &EventWidgets) {
    state.fetching_release = false;

    if state.recovery_urgent {
        widgets.recovery.options[RECOVERY_PARTITION].show_button();
        widgets.recovery.show();
    }

    if state.refresh_found {
        widgets.recovery.options[REFRESH_OS].show_button();
        widgets.recovery.show();
    }

    if state.upgrade_found {
        widgets.upgrade.options[0].show_button();
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

    widgets.dismisser.foreach(WidgetExt::destroy);
    widgets.dismisser.add(widget.as_ref());
    widgets.dismisser.show_all();

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
            mut current,
            is_current,
            is_lts,
            reboot_ready,
            refresh,
            status_failed,
            upgrade_text,
            upgrade,
            upgrading_recovery,
            urgent,
        } => {
            state.upgrade_label = upgrade_text;
            state.upgrade_version = upgrade;
            state.refresh_found = refresh;

            if let Some(release) = current.take() {
                state.recovery_urgent = dbg!(urgent);
                state.current = release;
            }

            if is_current {
                widgets.upgrade.disable(0, "You are running the most current Pop!_OS version");
            } else if status_failed {
                widgets.upgrade.disable(0, "Failed to check for upgrade status");
            } else {
                connect_upgrade(state, widgets, is_lts, reboot_ready);
            }

            if refresh {
                widgets.recovery.show();
                connect_refresh(&state, widgets);
                recovery::update_status(&state, widgets, status_failed, upgrading_recovery);
            } else {
                widgets.recovery.hide();
            }

            widgets.container.show();
        }
    }
}

/// Creates the upgrade signal for the upgrade button.
fn upgrade_action(sender: sync::Weak<glib::Sender<UiEvent>>) -> (&'static str, Box<dyn Fn()>) {
    let action: Box<dyn Fn()> = Box::new(move || {
        if let Some(sender) = sender.upgrade() {
            let _ = sender.send(UiEvent::Upgrade(OsUpgradeEvent::Dialog));
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

        widgets.upgrade.options[0].label("Preparing Upgrade").show_progress();

        widgets.recovery.options[RECOVERY_PARTITION].sensitive(false);
        widgets.recovery.options[REFRESH_OS].sensitive(false);

        if let Some(dismisser) = state.dismisser.take() {
            dismisser.destroy();
        }

        let _ = state.sender.send(BackgroundEvent::DownloadUpgrade(info));
    }
}

mod recovery {
    use super::*;
    use crate::state::State;

    pub fn clicked(state: &mut State, widgets: &EventWidgets) {
        widgets.upgrade.options[0].sensitive(false);
        widgets.recovery.options[REFRESH_OS].sensitive(false);
        let _ = state.sender.send(BackgroundEvent::UpdateRecovery(state.current.clone()));
    }

    pub fn update_status(
        state: &State,
        widgets: &EventWidgets,
        status_failed: bool,
        upgrading: bool,
    ) {
        let recovery_option = &widgets.recovery.options[RECOVERY_PARTITION];

        let allow_refresh = if state.recovery_urgent {
            let signal = Some((
                "Update",
                Box::new(enclose!((state.gui_sender => sender) move || {
                    if let Some(sender) = sender.upgrade() {
                        let _ = sender.send(UiEvent::Recovery(OsRecoveryEvent::Update));
                    }
                })),
            ));

            recovery_option
                .label("Recovery partition update is available")
                .sublabel(None)
                .button_signal(signal);

            false
        } else if upgrading {
            if let Some(sender) = state.gui_sender.upgrade() {
                let _ = sender.send(UiEvent::Initiated(InitiatedEvent::Recovery));
            }

            widgets.upgrade.options[0].sensitive(false);
            widgets.recovery.options[REFRESH_OS].sensitive(false);
            true
        } else if status_failed {
            recovery_option
                .label("Recovery Partition")
                .sublabel(Some("Failed to check for recovery updates"))
                .hide_widgets();
            true
        } else {
            recovery_option
                .label("Recovery Partition")
                .sublabel(Some("You have the most current version of the recovery version"))
                .hide_widgets();
            true
        };

        widgets.recovery.options[REFRESH_OS].sensitive(allow_refresh);
    }
}
