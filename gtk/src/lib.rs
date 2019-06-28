#[macro_use]
extern crate cascade;
#[macro_use]
extern crate err_derive;
#[macro_use]
extern crate shrinkwraprs;

mod widgets;

use self::widgets::*;
use gtk::prelude::*;
use num_traits::cast::FromPrimitive;
use pop_upgrade::{
    client::{self, Client, Error, ReleaseInfo, Signal},
    daemon::DaemonStatus,
    recovery::ReleaseFlags,
    release::{self, RefreshOp, UpgradeMethod},
};
use std::{
    borrow::Cow,
    path::Path,
    process::Command,
    rc::Rc,
    sync::{mpsc, Arc},
    thread,
};

#[derive(Shrinkwrap)]
pub struct UpgradeWidget {
    sender: Arc<mpsc::SyncSender<BackgroundEvent>>,
    #[shrinkwrap(main_field)]
    container: gtk::Container,
}

impl UpgradeWidget {
    pub fn new() -> Self {
        let (bg_sender, bg_receiver) = mpsc::sync_channel(5);
        let (gui_sender, gui_receiver) = glib::MainContext::channel(glib::PRIORITY_DEFAULT);
        let bg_sender = Arc::new(bg_sender);

        Self::background_event_loop(bg_receiver, gui_sender);

        let option_upgrade = UpgradeOption::new();
        let option_refresh = UpgradeOption::new();

        cascade! {
            gtk::SizeGroup::new(gtk::SizeGroupMode::Both);
            ..add_widget(&option_upgrade.button);
            ..add_widget(&option_refresh.button);
        }

        cascade! {
            gtk::SizeGroup::new(gtk::SizeGroupMode::Both);
            ..add_widget(option_upgrade.as_ref());
            ..add_widget(option_refresh.as_ref());
        }

        option_refresh
            .set_label("Refresh OS")
            .set_sublabel("Reinstall while keeping user accounts and files".into());

        let options = cascade! {
            gtk::ListBox::new();
            ..set_selection_mode(gtk::SelectionMode::None);
            ..insert(option_upgrade.as_ref(), -1);
            ..insert(option_refresh.as_ref(), -1);
            ..show();
        };

        let container = cascade! {
            gtk::Box::new(gtk::Orientation::Vertical, 12);
            ..add(&cascade! {
                gtk::Label::new("<b>OS Upgrade &amp; Refresh</b>");
                ..set_use_markup(true);
                ..set_xalign(0.0);
                ..show();
            });
            ..add(&cascade! {
                gtk::Frame::new(None);
                ..add(&options);
                ..show();
            });
            ..show();
        };

        {
            let container = container.clone();
            let sender = Arc::downgrade(&bg_sender);
            gui_receiver.attach(None, move |event| {
                match event {
                    UiEvent::ProgressRecovery(progress, total)
                    | UiEvent::ProgressUpgrade(progress, total) => {
                        option_upgrade.progress(progress, total);
                    }
                    UiEvent::Quit => return glib::Continue(false),
                    UiEvent::CommencedRefresh => {
                        option_upgrade.hide();
                    }
                    UiEvent::CommencedScanning => {
                        container.hide();
                        option_refresh.hide();
                    }
                    UiEvent::CommencedRecovery => {
                        option_refresh.hide();
                        option_upgrade.progress_view();
                        option_upgrade.progress_label("Upgrading recovery partition");
                    }
                    UiEvent::CommencedUpgrade => {
                        option_refresh.hide();
                        option_upgrade.progress_view();
                        option_upgrade.progress_label("Preparing to upgrade OS");
                    }
                    UiEvent::CompleteRecovery => {
                        eprintln!("successfully upgraded recovery partition");
                    }
                    UiEvent::CompleteRefresh | UiEvent::CompleteUpgrade => {
                        let _ = Command::new("systemctl").arg("reboot").status();
                    }
                    UiEvent::CompleteScan(upgrade_text, upgrade, refresh) => {
                        option_upgrade
                            .set_label(&upgrade_text)
                            .set_sublabel(None)
                            .set_button(if let Some(info) = upgrade {
                                let sender = sender.clone();
                                let action = move || {
                                    if let Some(sender) = sender.upgrade() {
                                        sender.send(BackgroundEvent::UpgradeOS(info.clone()));
                                    }
                                };
                                Some(("", action))
                            } else {
                                None
                            })
                            .show();

                        if refresh {
                            let sender = sender.clone();
                            let action = move || {
                                if let Some(sender) = sender.upgrade() {
                                    sender.send(BackgroundEvent::RefreshOS);
                                }
                            };

                            option_refresh.set_button(Some(("Refresh", action))).show();
                        }

                        container.show();
                    }
                    UiEvent::StatusChanged(from, to, why) => {
                        eprintln!("status changed from {} to {}: {}", from, to, why);
                        if let Some(sender) = sender.upgrade() {
                            sender.send(BackgroundEvent::GetStatus(from));
                        }
                    }
                    UiEvent::Error(why) => {
                        eprintln!("{}", why);
                    }
                }
                glib::Continue(true)
            });
        }

        Self {
            container: container.upcast::<gtk::Container>(),
            sender: bg_sender,
        }
    }

    pub fn scan(&self) {
        self.hide();
        let _ = self.sender.send(BackgroundEvent::Scan);
    }

    pub fn upgrade_daemon_is_active(&self) -> bool {
        let (tx, rx) = mpsc::sync_channel(0);
        let _ = self.sender.send(BackgroundEvent::IsActive(tx));
        rx.recv().unwrap_or(false)
    }

    fn background_event_loop(
        receiver: mpsc::Receiver<BackgroundEvent>,
        sender: glib::Sender<UiEvent>,
    ) {
        thread::spawn(move || {
            if let Ok(ref client) = Client::new() {
                for event in receiver.recv() {
                    match event {
                        BackgroundEvent::GetStatus(from) => {
                            get_status(client, &sender, from);
                        }
                        BackgroundEvent::IsActive(tx) => {
                            let _ = tx.send(client.status().is_ok());
                        }
                        BackgroundEvent::RefreshOS => {
                            refresh_os(client, &sender);
                        }
                        BackgroundEvent::Scan => scan(client, &sender),
                        BackgroundEvent::UpgradeOS(info) => {
                            upgrade_os(client, &sender, info);
                        }
                        BackgroundEvent::Quit => {
                            eprintln!("stopping background thread");
                            break;
                        }
                    }
                }
            }
        });
    }
}

/// Events sent to this widget's background thread.
enum BackgroundEvent {
    GetStatus(DaemonStatus),
    IsActive(mpsc::SyncSender<bool>),
    RefreshOS,
    Scan,
    UpgradeOS(ReleaseInfo),
    Quit,
}

/// Events received for the UI to handle.
enum UiEvent {
    CommencedRecovery,
    CommencedRefresh,
    CommencedUpgrade,
    CommencedScanning,
    CompleteRecovery,
    CompleteRefresh,
    CompleteUpgrade,
    CompleteScan(Box<str>, Option<ReleaseInfo>, bool),
    Error(UiError),
    ProgressRecovery(u64, u64),
    ProgressUpgrade(u64, u64),
    StatusChanged(DaemonStatus, DaemonStatus, Box<str>),
    Quit,
}

#[derive(Debug, Error)]
enum UiError {
    #[error(display = "recovery upgrade failed")]
    Recovery(#[error(cause)] UnderlyingError),
    #[error(display = "failed to set up OS refresh")]
    Refresh(#[error(cause)] UnderlyingError),
    #[error(display = "failed to upgrade OS")]
    Upgrade(#[error(cause)] UnderlyingError),
}

#[derive(Debug, Error)]
#[error(display = "{}", _0)]
struct StatusError(Box<str>);

#[derive(Debug, Error)]
enum UnderlyingError {
    #[error(display = "client error")]
    Client(#[error(cause)] Error),
    #[error(display = "failed status")]
    Status(#[error(cause)] StatusError),
}

impl From<Box<str>> for UnderlyingError {
    fn from(why: Box<str>) -> Self {
        UnderlyingError::Status(StatusError(why))
    }
}

impl From<Error> for UnderlyingError {
    fn from(why: Error) -> Self {
        UnderlyingError::Client(why)
    }
}

fn scan(client: &Client, sender: &glib::Sender<UiEvent>) {
    let _ = sender.send(UiEvent::CommencedScanning);
    let mut upgrade_text = Cow::Borrowed("No upgrades available");
    let mut upgrade = None;

    if release::upgrade_in_progress() {
        upgrade_text = Cow::Borrowed("Release upgrade already occuring");
    } else {
        if let Ok(info) = client.release_check() {
            if info.build > 0 {
                eprintln!(
                    "upgrade from {} to {} is available",
                    info.current, info.next
                );

                upgrade_text =
                    Cow::Owned(format!("Upgrade from {} to {}", info.current, info.next));
                upgrade = Some(info);
            }
        }
    }

    let _ = sender.send(UiEvent::CompleteScan(
        Box::from(upgrade_text.as_ref()),
        upgrade,
        client.recovery_exists(),
    ));
}

fn get_status(client: &Client, sender: &glib::Sender<UiEvent>, from: DaemonStatus) {
    match from {
        DaemonStatus::RecoveryUpgrade => {
            let event = match client.recovery_upgrade_release_status() {
                Ok(status) => {
                    if status.status == 0 {
                        UiEvent::CompleteRecovery
                    } else {
                        UiEvent::Error(UiError::Recovery(status.why.into()))
                    }
                }
                Err(why) => UiEvent::Error(UiError::Recovery(why.into())),
            };

            let _ = sender.send(event);
        }
        DaemonStatus::ReleaseUpgrade => {
            let event = match client.release_upgrade_status() {
                Ok(status) => {
                    if status.status == 0 {
                        UiEvent::CompleteUpgrade
                    } else {
                        UiEvent::Error(UiError::Upgrade(status.why.into()))
                    }
                }
                Err(why) => UiEvent::Error(UiError::Upgrade(why.into())),
            };

            let _ = sender.send(event);
        }
        _ => (),
    }
}

fn refresh_os(client: &Client, sender: &glib::Sender<UiEvent>) {
    let _ = sender.send(UiEvent::CommencedRefresh);

    if let Err(why) = client.refresh_os(RefreshOp::Enable) {
        let _ = sender.send(UiEvent::Error(UiError::Refresh(why.into())));
        return;
    }

    let _ = sender.send(UiEvent::CompleteRefresh);
}

fn upgrade_os(client: &Client, sender: &glib::Sender<UiEvent>, info: ReleaseInfo) {
    let &ReleaseInfo {
        ref current,
        ref next,
        ..
    } = &info;

    let how = if client.recovery_exists() {
        // Upgrade the recovery partition in addition to the OS.
        if !upgrade_recovery(client, sender, next) {
            return;
        }

        UpgradeMethod::Recovery
    } else {
        UpgradeMethod::Offline
    };

    let _ = sender.send(UiEvent::CommencedUpgrade);

    if let Err(why) = client.release_upgrade(how, current, next) {
        let _ = sender.send(UiEvent::Error(UiError::Upgrade(why.into())));
        return;
    }

    let error = &mut None;

    client.event_listen(
        DaemonStatus::ReleaseUpgrade,
        Client::release_upgrade_status,
        |new_status| {
            let status =
                DaemonStatus::from_u8(new_status.status).expect("unknown daemon status value");
            let _ = sender.send(UiEvent::StatusChanged(
                DaemonStatus::ReleaseUpgrade,
                status,
                new_status.why,
            ));
        },
        |client, signal| {
            match signal {
                Signal::PackageFetchResult(status) => {
                    if status.status != 0 {
                        *error = Some(status.why);
                    }

                    return Ok(client::Continue(false));
                }
                Signal::PackageFetched(package) => {
                    let _ = sender.send(UiEvent::ProgressUpgrade(
                        package.completed as u64,
                        package.total as u64,
                    ));
                }
                _ => (),
            }

            Ok(client::Continue(true))
        },
    );

    if let Some(why) = error.take() {
        let _ = sender.send(UiEvent::Error(UiError::Upgrade(why.into())));
        return;
    }

    let _ = sender.send(UiEvent::CompleteUpgrade);
}

fn upgrade_recovery(client: &Client, sender: &glib::Sender<UiEvent>, version: &str) -> bool {
    let _ = sender.send(UiEvent::CommencedRecovery);

    let arch = "nvidia";
    let flags = ReleaseFlags::empty();

    if let Err(why) = client.recovery_upgrade_release(version, arch, flags) {
        let _ = sender.send(UiEvent::Error(UiError::Recovery(why.into())));
        return false;
    }

    let error = &mut None;

    client.event_listen(
        DaemonStatus::RecoveryUpgrade,
        Client::recovery_upgrade_release_status,
        |new_status| {
            let status =
                DaemonStatus::from_u8(new_status.status).expect("unknown daemon status value");
            let _ = sender.send(UiEvent::StatusChanged(
                DaemonStatus::RecoveryUpgrade,
                status,
                new_status.why,
            ));
        },
        |client, signal| {
            match signal {
                Signal::RecoveryDownloadProgress(progress) => {
                    let _ =
                        sender.send(UiEvent::ProgressRecovery(progress.progress, progress.total));
                }
                Signal::RecoveryResult(status) => {
                    if status.status != 0 {
                        *error = Some(status.why);
                    }

                    return Ok(client::Continue(false));
                }
                _ => (),
            }

            Ok(client::Continue(true))
        },
    );

    if let Some(why) = error.take() {
        let _ = sender.send(UiEvent::Error(UiError::Recovery(why.into())));
        return false;
    }

    let _ = sender.send(UiEvent::CompleteRecovery);
    true
}
