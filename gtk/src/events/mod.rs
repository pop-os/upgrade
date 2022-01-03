pub mod background;

pub use self::background::{scan::ScanEvent, BackgroundEvent};

use crate::{
    errors::UiError,
    fl, get_dismiss_row, get_upgrade_row, notify, reboot,
    state::State,
    widgets::{
        dialogs::{RefreshDialog, UpgradeDialog},
        permissions::PermissionDenied,
        Dismisser, Section,
    },
    RECOVERY_PARTITION, REFRESH_OS,
};

use chrono::{TimeZone, Utc};
use gtk::prelude::*;

use pop_upgrade::{
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
    Initiated(InitiatedEvent),
    Progress(ProgressEvent),
    Recovery(OsRecoveryEvent),
    Shutdown,
    StatusChanged(DaemonStatus, DaemonStatus, Box<str>),
    Updated,
    Updating,
    // UpgradeClicked,
    // UpgradeEvent(UpgradeEvent),
    // UpgradeNotificationClicked,
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
    pub button_sg:     gtk::SizeGroup,
    pub container:     gtk::Box,
    pub dismisser:     gtk::ListBoxRow,
    pub stack:         gtk::Stack,
    pub loading_label: gtk::Label,

    pub recovery: Section,
    pub upgrade:  Section,
}

impl EventWidgets {
    /// Sets the upgrade frame to display a permission denied widget.
    fn permission_denied(&self) {
        cascade! {
            &self.upgrade.frame;
            ..remove(&self.upgrade.list);
            ..add(PermissionDenied::new().as_ref());
            ..show_all();
        };
    }
}

pub fn attach(gui_receiver: flume::Receiver<UiEvent>, widgets: EventWidgets, mut state: State) {
    let event_handler = async move {
        while let Ok(event) = gui_receiver.recv_async().await {
            debug!("{:?}", event);
            match event {
                UiEvent::Progress(event) => match event {
                    ProgressEvent::Fetching(progress, total) => {
                        let progress = state.calculate_fetching_progress(progress, total);
                        widgets.upgrade.options[0].progress_exact(progress as u8).show_progress();
                    }

                    ProgressEvent::Recovery(progress, total) => {
                        widgets.recovery.options[RECOVERY_PARTITION]
                            .label(&fl!(
                                "recovery-progress",
                                current = (progress / 1024),
                                total = (total / 1024)
                            ))
                            .sublabel(None)
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
                            .label(&fl!("download-os", version = (&*version)))
                            .reset_progress()
                            .show_progress();

                        state.upgrading_to = version;
                    }

                    InitiatedEvent::Refresh => {
                        get_upgrade_row(&widgets.upgrade.list).hide();
                    }

                    InitiatedEvent::Scanning => {
                        widgets.upgrade.options[0].reset_progress();
                        widgets.loading_label.set_label(&fl!("checking-for-updates"));
                        widgets.stack.set_visible_child_name("loading");
                        widgets.recovery.options[RECOVERY_PARTITION].reset_progress();
                        widgets.upgrade.options[RECOVERY_PARTITION].hide_widgets();
                    }

                    InitiatedEvent::Recovery => {
                        widgets.recovery.options[RECOVERY_PARTITION]
                            .label(&fl!("recovery-downloading"))
                            .sublabel(None)
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

                UiEvent::Updating => {
                    widgets.loading_label.set_label(&fl!("daemon-updating"));
                    widgets.stack.set_visible_child_name("loading");
                }

                UiEvent::Updated => {
                    widgets.stack.set_visible_child_name("updated");
                }

                // Events pertaining to the recovery section
                UiEvent::Recovery(event) => match event {
                    OsRecoveryEvent::Event(event) => match event {
                        RecoveryEvent::Verifying => {
                            widgets.recovery.options[RECOVERY_PARTITION]
                                .label(&fl!("recovery-verify"))
                                .hide_widgets();
                        }
                        RecoveryEvent::Syncing => {
                            widgets.recovery.options[RECOVERY_PARTITION]
                                .label(&fl!("recovery-sync"));
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
                            .label(&fl!("recovery-header"))
                            .sublabel(Some(&fl!("most-current-recovery")))
                            .hide_widgets();
                    }

                    CompletedEvent::Refresh => reboot(),

                    CompletedEvent::Scan(event) => {
                        widgets.stack.set_visible_child_name("updated");
                        scan_event(&mut state, &widgets, event);
                    }
                },

                UiEvent::StatusChanged(from, to, why) => {
                    warn!("status changed from {} to {}: {}", from, to, why);
                    let _ = state.sender.send(BackgroundEvent::GetStatus(from));
                }

                UiEvent::Error(why) => error(&mut state, &widgets, &why),

                UiEvent::WaitingOnLock => (),

                UiEvent::Shutdown => return,
            }
        }
    };

    glib::MainContext::default().spawn_local(event_handler);
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

    widgets.recovery.options[REFRESH_OS]
        .button_signal(Some((fl!("button-refresh"), action)))
        .show();
}

/// Programs the upgrade button, and optionally enables the dismissal widget.
fn connect_upgrade(state: &mut State, widgets: &EventWidgets, is_lts: bool, reboot_ready: bool) {
    let notice = match EolDate::fetch() {
        Ok(eol) => {
            let (y, m, d) = eol.ymd;
            match eol.status() {
                EolStatus::Exceeded => Some(fl!(
                    "eol-exceeded",
                    current = fomat!((eol.version)),
                    next = fomat!((eol.version.next_release()))
                )),
                EolStatus::Imminent => Some(fl!(
                    "eol-imminent",
                    current = fomat!((eol.version)),
                    date = fomat!((Utc.ymd(y as i32, m, d).format("%B %-d, %Y")))
                )),
                EolStatus::Ok => None,
            }
        }
        Err(why) => {
            error!("{}: {}", fl!("eol-error"), why);
            None
        }
    };

    let notice = notice.as_deref();

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
fn download_action(sender: sync::Weak<flume::Sender<UiEvent>>) -> (String, Box<dyn Fn()>) {
    let action: Box<dyn Fn()> = Box::new(move || {
        if let Some(sender) = sender.upgrade() {
            let _ = sender.send(UiEvent::Upgrade(OsUpgradeEvent::Upgrade));
        }
    });

    (fl!("button-download"), action)
}

/// Notify that OS release updates have been downloaded, and are ready to commence.
fn download_complete(state: &mut State, widgets: &EventWidgets) {
    state.upgrade_downloaded = true;

    let description = fl!("notification-description", version = (&*state.upgrading_to));
    thread::spawn(enclose!((state.gui_sender => sender) move || {
        notify::notify("distributor-logo", &fl!("notification-title"), &description, || {
            if let Some(sender) = sender.upgrade() {
                let _ = sender.send(UiEvent::Upgrade(OsUpgradeEvent::Notification));
            }
        });
    }));

    (state.callback_event.borrow())(Event::UpgradeReady);

    widgets.upgrade.options[0]
        .show_button()
        .button_label(&fl!("button-upgrade"))
        .label(&fl!("download-os-complete", version = (&*state.upgrading_to)));
}

use once_cell::sync::Lazy;

const GENERIC: Lazy<String> = Lazy::new(|| {
    fomat!(
        (fl!("error-header")) "\n\n"
        "* /etc/apt/sources.list\n"
        "* /etc/apt/sources.list.d/\n"
        "* /etc/fstab\n\n"
        (fl!("error-collect-logs")) "\n\n"
        (fl!("error-package-manager")) "\n\n"
        "sudo apt clean\n"
        "sudo apt update -m\n"
        "sudo dpkg --configure -a\n"
        "sudo apt install -f\n"
        "sudo apt dist-upgrade\n"
        "sudo apt autoremove --purge\n"
    )
});

/// Formats error messages for display on the console, and in the UI.
fn error(state: &mut State, widgets: &EventWidgets, why: &UiError) {
    let error_message = &mut format!("{}", why);
    why.iter_sources().for_each(|source| {
        error_message.push_str(": ");
        error_message.push_str(format!("{}", source).as_str());
    });

    if let UiError::Recovery(ref why) = why {
        widgets.recovery.options[RECOVERY_PARTITION]
            .label(&fl!("error-recovery-download"))
            .sublabel(Some(&fl!("error-try-again")))
            .hide_widgets();
        (state.callback_error.borrow())(
            format!("{}:\n\n{:#?}", fl!("error-recovery-update"), why).as_str(),
        );
    } else {
        (state.callback_error.borrow())(
            &fomat!((&*GENERIC) "\n\n" (fl!("error-originating-cause")) "\n\n" (error_message)),
        );
    }

    error!("{}", error_message);

    if let UiError::Dismiss(dismissed, _) = why {
        if let Some(dismisser) = state.dismisser.as_mut() {
            dismisser.set_dismissed(!dismissed);
        }
    } else {
        (state.callback_event.borrow())(Event::NotUpgrading);
        reset(state, widgets);
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
        widgets.upgrade.options[0].label(&fl!("upgrade-canceling"));
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
                state.recovery_urgent = urgent;
                state.current = release;
            }

            if is_current {
                widgets.upgrade.disable(0, &fl!("release-current"));
            } else if status_failed {
                widgets.upgrade.disable(0, &fl!("error-upgrade-status"));
            } else {
                connect_upgrade(state, widgets, is_lts, reboot_ready);
            }

            if refresh {
                widgets.recovery.show();
                connect_refresh(state, widgets);
                recovery::update_status(state, widgets, status_failed, upgrading_recovery);
            } else {
                widgets.recovery.hide();
            }

            widgets.container.show();
        }
    }
}

/// Creates the upgrade signal for the upgrade button.
fn upgrade_action(sender: sync::Weak<flume::Sender<UiEvent>>) -> (String, Box<dyn Fn()>) {
    let action: Box<dyn Fn()> = Box::new(move || {
        if let Some(sender) = sender.upgrade() {
            let _ = sender.send(UiEvent::Upgrade(OsUpgradeEvent::Dialog));
        }
    });

    (fl!("button-upgrade"), action)
}

/// Triggers on clicking the upgrade button
fn upgrade_clicked(state: &mut State, widgets: &EventWidgets) {
    if state.upgrade_downloaded {
        release_upgrade_dialog(state, widgets);
        return;
    }

    if let Some(info) = state.upgrade_version.clone() {
        (state.callback_event.borrow())(Event::Upgrading);

        widgets.upgrade.options[0].label(&fl!("upgrade-preparing")).show_progress();

        widgets.recovery.options[RECOVERY_PARTITION].sensitive(false);
        widgets.recovery.options[REFRESH_OS].sensitive(false);

        if let Some(dismisser) = state.dismisser.take() {
            unsafe {
                dismisser.destroy();
            }
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
                fl!("button-update"),
                Box::new(enclose!((state.gui_sender => sender) move || {
                    if let Some(sender) = sender.upgrade() {
                        let _ = sender.send(UiEvent::Recovery(OsRecoveryEvent::Update));
                    }
                })),
            ));

            recovery_option
                .label(&fl!("recovery-update-found"))
                .sublabel(None)
                .button_signal(signal);

            false
        } else if upgrading {
            widgets.upgrade.options[0].sensitive(false);
            widgets.recovery.options[REFRESH_OS].sensitive(false);

            true
        } else if status_failed {
            recovery_option
                .label(&fl!("recovery-header"))
                .sublabel(Some(&fl!("error-recovery-check")))
                .hide_widgets();
            true
        } else {
            recovery_option
                .label(&fl!("recovery-header"))
                .sublabel(Some(&fl!("most-current-recovery")))
                .hide_widgets();
            true
        };

        widgets.recovery.options[REFRESH_OS].sensitive(allow_refresh);
    }
}
