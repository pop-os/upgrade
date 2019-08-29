#[macro_use]
extern crate cascade;
#[macro_use]
extern crate err_derive;
#[macro_use]
extern crate shrinkwraprs;

mod widgets;

use self::widgets::*;
use apt_cli_wrappers::AptUpgradeEvent;
use gtk::prelude::*;
use num_traits::cast::FromPrimitive;
use pop_upgrade::{
    client::{self, Client, Error, ReleaseInfo, RepoCompatError, Signal, Status},
    daemon::DaemonStatus,
    recovery::ReleaseFlags,
    release::{self, RefreshOp, UpgradeMethod},
};
use std::{
    borrow::Cow,
    cell::RefCell,
    error::Error as ErrorTrait,
    process::Command,
    rc::Rc,
    sync::{mpsc, Arc},
    thread,
};

pub type ErrorCallback = Rc<RefCell<Box<dyn Fn(&str)>>>;

#[derive(Shrinkwrap)]
pub struct UpgradeWidget {
    sender: Arc<mpsc::SyncSender<BackgroundEvent>>,
    callback_error: ErrorCallback,
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
                gtk::LabelBuilder::new()
                    .label("<b>OS Upgrade &amp; Refresh</b>")
                    .use_markup(true)
                    .xalign(0.0)
                    .build();
                ..show();
            });
            ..add(&cascade! {
                gtk::Frame::new(None);
                ..add(&options);
                ..show();
            });
            ..show();
        };

        let callback_error: ErrorCallback = Rc::new(RefCell::new(Box::new(|_| ())));

        {
            let container = container.clone();
            let sender = bg_sender.clone();
            let mut refresh_found = false;
            let mut upgrade_found = false;
            let callback_error = Rc::downgrade(&callback_error);
            gui_receiver.attach(None, move |event| {
                eprintln!("received event: {:?}", event);
                match event {
                    UiEvent::ProgressFetching(progress, total)
                    | UiEvent::ProgressRecovery(progress, total)
                    | UiEvent::ProgressUpgrade(progress, total) => {
                        option_upgrade.progress(progress, total);
                    }
                    UiEvent::ProgressUpdates(percent) => {
                        option_upgrade
                            .progress_exact(percent)
                            .progress_label("Upgrading packages for current release");
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
                        option_upgrade
                            .progress_view()
                            .progress_label("Upgrading recovery partition");
                    }
                    UiEvent::CommencedUpgrade => {
                        option_refresh.hide();
                        option_upgrade
                            .progress_view()
                            .progress_label("Preparing to upgrade OS")
                            .show();
                    }
                    UiEvent::CompleteRecovery => {
                        eprintln!("successfully upgraded recovery partition");
                    }
                    UiEvent::CompleteRefresh | UiEvent::CompleteUpgrade => {
                        let _ = Command::new("systemctl").arg("reboot").status();
                    }
                    UiEvent::CompleteScan(upgrade_text, upgrade, refresh) => {
                        upgrade_found = upgrade.is_some();
                        refresh_found = refresh;

                        option_upgrade
                            .set_label(&upgrade_text)
                            .set_sublabel(None)
                            .set_button(if let Some(info) = upgrade {
                                let sender = Arc::downgrade(&sender);
                                let action = move || {
                                    if let Some(sender) = sender.upgrade() {
                                        let _ =
                                            sender.send(BackgroundEvent::UpgradeOS(info.clone()));
                                    }
                                };
                                Some(("Upgrade", action))
                            } else {
                                None
                            })
                            .show();

                        if refresh {
                            let sender = Arc::downgrade(&sender);
                            let action = move || {
                                if let Some(sender) = sender.upgrade() {
                                    let _ = sender.send(BackgroundEvent::RefreshOS);
                                }
                            };

                            option_refresh.set_button(Some(("Refresh", action))).show();
                        }

                        container.show();
                    }
                    UiEvent::IncompatibleRepos(repos) => {
                        let failures = repos
                            .failure
                            .into_iter()
                            .map(|(repo, why)| {
                                eprintln!("cannot upgrade {}: {}", repo, why);
                                Box::from(repo)
                            })
                            .collect::<Vec<Box<str>>>();

                        let dialog = RepositoryDialog::new(failures.iter());

                        if gtk::ResponseType::Accept == dialog.run() {
                            let _ = sender.send(BackgroundEvent::RepoModify(
                                failures,
                                dialog.answers().collect::<Vec<bool>>(),
                            ));
                        }

                        dialog.destroy();
                    }
                    UiEvent::StatusChanged(from, to, why) => {
                        eprintln!("status changed from {} to {}: {}", from, to, why);
                        let _ = sender.send(BackgroundEvent::GetStatus(from));
                    }
                    UiEvent::Updates(total) => {
                        option_refresh.hide();
                        option_upgrade
                            .progress_view()
                            .progress_label(&format!("Fetching {} packages", total));
                    }
                    UiEvent::Error(why) => {
                        if refresh_found {
                            option_upgrade.button_view().show();
                        }

                        if upgrade_found {
                            option_upgrade.button_view().show();
                        }

                        let error_message = &mut format!("{}", why);
                        why.iter_sources().for_each(|source| {
                            error_message.push_str(": ");
                            error_message.push_str(format!("{}", source).as_str());
                        });
                        let error_message = error_message.as_str();

                        if let Some(callback) = callback_error.upgrade() {
                            (*callback.borrow())(error_message);
                        }

                        eprintln!("{}", error_message);
                    }
                }
                glib::Continue(true)
            });
        }

        Self { container: container.upcast::<gtk::Container>(), sender: bg_sender, callback_error }
    }

    pub fn scan(&self) {
        self.hide();
        let _ = self.sender.send(BackgroundEvent::Scan);
    }

    pub fn callback_error<F: Fn(&str) + 'static>(&self, func: F) {
        *self.callback_error.borrow_mut() = Box::from(func);
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
            let sender = &sender;
            if let Ok(ref client) = Client::new() {
                while let Ok(event) = receiver.recv() {
                    match event {
                        BackgroundEvent::GetStatus(from) => {
                            get_status(client, sender, from);
                        }
                        BackgroundEvent::IsActive(tx) => {
                            let _ = tx.send(client.status().is_ok());
                        }
                        BackgroundEvent::RefreshOS => {
                            refresh_os(client, sender);
                        }
                        BackgroundEvent::RepoModify(failures, answers) => {
                            repo_modify(client, sender, failures, answers);
                        }
                        BackgroundEvent::Scan => scan(client, sender),
                        BackgroundEvent::UpgradeOS(info) => {
                            upgrade_os(client, sender, info);
                        }
                        BackgroundEvent::Quit => {
                            eprintln!("stopping background thread");
                            break;
                        }
                    }
                }
            }

            eprintln!("breaking free");
        });
    }
}

/// Events sent to this widget's background thread.
#[derive(Debug)]
enum BackgroundEvent {
    GetStatus(DaemonStatus),
    IsActive(mpsc::SyncSender<bool>),
    RefreshOS,
    RepoModify(Vec<Box<str>>, Vec<bool>),
    Scan,
    UpgradeOS(ReleaseInfo),
    Quit,
}

/// Events received for the UI to handle.
#[derive(Debug)]
enum UiEvent {
    CommencedRecovery,
    CommencedRefresh,
    CommencedUpgrade,
    CommencedScanning,
    CompleteRecovery,
    CompleteRefresh,
    CompleteUpgrade,
    CompleteScan(Box<str>, Option<ReleaseInfo>, bool),
    IncompatibleRepos(RepoCompatError),
    Error(UiError),
    ProgressFetching(u64, u64),
    ProgressRecovery(u64, u64),
    ProgressUpdates(u8),
    ProgressUpgrade(u64, u64),
    StatusChanged(DaemonStatus, DaemonStatus, Box<str>),
    Updates(u32),
    Quit,
}

#[derive(Debug, Error)]
enum UiError {
    #[error(display = "recovery upgrade failed")]
    Recovery(#[error(cause)] UnderlyingError),
    #[error(display = "failed to set up OS refresh")]
    Refresh(#[error(cause)] UnderlyingError),
    #[error(display = "failed to modify repos")]
    Repos(#[error(cause)] UnderlyingError),
    #[error(display = "failed to update system")]
    Updates(#[error(cause)] UnderlyingError),
    #[error(display = "failed to upgrade OS")]
    Upgrade(#[error(cause)] UnderlyingError),
}

impl UiError {
    pub fn iter_sources(&self) -> ErrorIter<'_> { ErrorIter { current: self.source() } }
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
    fn from(why: Box<str>) -> Self { UnderlyingError::Status(StatusError(why)) }
}

impl From<Error> for UnderlyingError {
    fn from(why: Error) -> Self { UnderlyingError::Client(why) }
}

fn scan(client: &Client, sender: &glib::Sender<UiEvent>) {
    eprintln!("scanning");
    let _ = sender.send(UiEvent::CommencedScanning);
    let mut upgrade_text = Cow::Borrowed("No upgrades available");
    let mut upgrade = None;

    if release::upgrade_in_progress() {
        upgrade_text = Cow::Borrowed("Release upgrade already occuring");
    } else {
        if let Ok(info) = client.release_check() {
            if info.build > 0 {
                eprintln!("upgrade from {} to {} is available", info.current, info.next);

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

fn repo_modify(
    client: &Client,
    sender: &glib::Sender<UiEvent>,
    failures: Vec<Box<str>>,
    answers: Vec<bool>,
) {
    let input = failures.into_iter().zip(answers.into_iter());
    if let Err(why) = client.repo_modify(input) {
        let _ = sender.send(UiEvent::Error(UiError::Repos(why.into())));
        return;
    }
}

fn status_changed(sender: &glib::Sender<UiEvent>, new_status: Status, expected: DaemonStatus) {
    let status = DaemonStatus::from_u8(new_status.status).expect("unknown daemon status value");
    let _ = sender.send(UiEvent::StatusChanged(expected, status, new_status.why));
}

fn update_system(client: &Client, sender: &glib::Sender<UiEvent>) -> bool {
    eprintln!("checking if updates are required");
    let updates = match client.fetch_updates(Vec::new(), false) {
        Ok(updates) => updates,
        Err(why) => {
            let _ = sender.send(UiEvent::Error(UiError::Updates(why.into())));
            return false;
        }
    };

    if updates.updates_available {
        let _ = sender.send(UiEvent::Updates(updates.total));

        let error = &mut None;

        eprintln!("listening for package fetching signals");
        client.event_listen(
            DaemonStatus::FetchingPackages,
            Client::fetch_updates_status,
            |status| status_changed(sender, status, DaemonStatus::FetchingPackages),
            |client, signal| {
                match signal {
                    Signal::PackageFetchResult(status) => {
                        if status.status != 0 {
                            *error = Some(status.why);
                        }

                        return Ok(client::Continue(false));
                    }
                    Signal::PackageFetched(status) => {
                        let _ = sender.send(UiEvent::ProgressFetching(
                            status.completed as u64,
                            status.total as u64,
                        ));
                    }
                    Signal::PackageUpgrade(event) => {
                        if let Ok(AptUpgradeEvent::Progress { percent }) =
                            AptUpgradeEvent::from_dbus_map(event.into_iter())
                        {
                            let _ = sender.send(UiEvent::ProgressUpdates(percent));
                        }
                    }
                    _ => (),
                }

                Ok(client::Continue(true))
            },
        );

        if let Some(why) = error.take() {
            let _ = sender.send(UiEvent::Error(UiError::Updates(why.into())));
            return false;
        }
    }

    true
}

fn upgrade_os(client: &Client, sender: &glib::Sender<UiEvent>, info: ReleaseInfo) {
    eprintln!("upgrading OS");
    if !update_system(client, sender) {
        return;
    }

    let &ReleaseInfo { ref current, ref next, .. } = &info;

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
        |status| status_changed(sender, status, DaemonStatus::ReleaseUpgrade),
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
                Signal::RepoCompatError(repositories) => {
                    let _ = sender.send(UiEvent::IncompatibleRepos(repositories));
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
        |status| status_changed(sender, status, DaemonStatus::RecoveryUpgrade),
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

pub struct ErrorIter<'a> {
    current: Option<&'a (dyn ErrorTrait + 'static)>,
}

impl<'a> Iterator for ErrorIter<'a> {
    type Item = &'a (dyn ErrorTrait + 'static);

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.current;
        self.current = self.current.and_then(|ref why| why.source());
        current
    }
}
