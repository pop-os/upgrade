#[macro_use]
extern crate cascade;
#[macro_use]
extern crate err_derive;
#[macro_use]
extern crate log;
#[macro_use]
extern crate shrinkwraprs;

mod errors;
mod events;
mod notify;
mod widgets;

use self::{
    errors::*,
    events::*,
    widgets::{
        dialogs::{RepositoryDialog, UpgradeDialog},
        Dismisser, UpgradeOption,
    },
};
use apt_cli_wrappers::AptUpgradeEvent;
use gtk::prelude::*;
use num_traits::cast::FromPrimitive;
use pop_upgrade::{
    client::{self, Client, ReleaseInfo, Signal, Status},
    daemon::{DaemonStatus, DISMISSED},
    recovery::ReleaseFlags,
    release::{self, RefreshOp, UpgradeEvent, UpgradeMethod},
};
use std::{
    borrow::Cow,
    cell::RefCell,
    fs,
    path::Path,
    process::Command,
    rc::Rc,
    sync::{mpsc, Arc},
    thread,
};

pub type ErrorCallback = Rc<RefCell<Box<dyn Fn(&str)>>>;

#[derive(Shrinkwrap)]
pub struct UpgradeWidget {
    sender: mpsc::SyncSender<BackgroundEvent>,
    callback_error: ErrorCallback,
    #[shrinkwrap(main_field)]
    container: gtk::Container,
}

impl UpgradeWidget {
    pub fn new() -> Self {
        let (bg_sender, bg_receiver) = mpsc::sync_channel(5);
        let (gui_sender, gui_receiver) = glib::MainContext::channel(glib::PRIORITY_DEFAULT);
        let gui_sender = Arc::new(gui_sender);

        {
            let gui_sender = gui_sender.clone();
            Self::background_event_loop(bg_receiver, move |event| {
                let _ = gui_sender.send(event);
            });
        }

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
            .set_label("Refresh OS install")
            .set_sublabel("Reinstall while keeping user accounts and files".into());

        let options = cascade! {
            gtk::ListBox::new();
            ..set_selection_mode(gtk::SelectionMode::None);
            ..add(option_upgrade.as_ref());
            ..add(option_refresh.as_ref());
            ..show();
        };

        fn get_upgrade_row(options: &gtk::ListBox) -> gtk::ListBoxRow {
            options.get_row_at_index(0).expect("upgrade option is not at index 1")
        }

        fn get_refresh_row(options: &gtk::ListBox) -> gtk::ListBoxRow {
            options.get_row_at_index(1).expect("refresh option is not at index 1")
        }

        let dismisser_frame = gtk::Frame::new(None);

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
            ..add(&dismisser_frame);
            ..show();
        };

        let callback_error: ErrorCallback = Rc::new(RefCell::new(Box::new(|_| ())));

        {
            let container = container.clone();
            let sender = bg_sender.clone();

            let mut refresh_found = false;
            let mut upgrade_found = false;
            let mut upgrade_downloaded = false;
            let mut upgrade_version = None::<ReleaseInfo>;
            let mut upgrading_to: Box<str> = Box::from("");
            let mut dismisser = None::<Dismisser>;

            let gui_sender = Arc::downgrade(&gui_sender);
            let callback_error = Rc::downgrade(&callback_error);

            gui_receiver.attach(None, move |event| {
                eprintln!("received event: {:?}", event);
                match event {
                    UiEvent::Dismissed => {
                        if let Some(dismisser) = dismisser.take() {
                            dismisser.destroy();
                            dismisser_frame.hide();
                        }
                    }
                    UiEvent::UpgradeEvent(event) => {
                        use UpgradeEvent::*;

                        let message = match event {
                            UpdatingPackageLists => Some("Updating package lists"),
                            UpdatingSourceLists => Some("Updating source lists"),
                            FetchingPackages => Some("Fetching packages"),
                            UpgradingPackages => Some("Upgrading packages"),
                            InstallingPackages => Some("Installing packages"),
                            FetchingPackagesForNewRelease => {
                                Some("Fetching packages for new release")
                            }
                            _ => None,
                        };

                        if let Some(message) = message {
                            option_upgrade.progress_label(message);
                        }
                    }
                    UiEvent::Progress(ProgressEvent::Fetching(progress, total)) => {
                        option_upgrade.progress(progress, total).progress_label(&format!(
                            "Fetching packages: {} / {}",
                            progress, total
                        ));
                    }
                    UiEvent::Progress(ProgressEvent::Recovery(progress, total)) => {
                        option_upgrade.progress(progress, total).progress_label(&format!(
                            "Upgrading recovery: {} / {}",
                            progress, total
                        ));
                    }
                    UiEvent::Progress(ProgressEvent::Updates(percent)) => {
                        option_upgrade
                            .progress_exact(percent)
                            .progress_label(&format!("Upgrading packages: {}%", percent));
                    }
                    UiEvent::Shutdown => return glib::Continue(false),
                    UiEvent::Initiated(InitiatedEvent::Refresh) => {
                        get_upgrade_row(&options).hide();
                    }
                    UiEvent::Initiated(InitiatedEvent::Scanning) => {
                        container.hide();
                    }
                    UiEvent::Initiated(InitiatedEvent::Recovery) => {
                        get_refresh_row(&options).hide();
                        option_upgrade
                            .progress_label("Upgrading recovery partition")
                            .progress_exact(0);
                    }
                    UiEvent::Initiated(InitiatedEvent::Download(version)) => {
                        get_refresh_row(&options).hide();
                        option_upgrade
                            .set_label(&*["Downloading Pop!_OS ", &version].concat())
                            .progress_label("Downloading")
                            .progress_exact(0);
                        upgrading_to = version;
                    }
                    UiEvent::Completed(CompletedEvent::Recovery) => {
                        info!("successfully upgraded recovery partition");
                    }
                    UiEvent::Completed(CompletedEvent::Refresh) => {
                        reboot();
                    }
                    UiEvent::Completed(CompletedEvent::Download) => {
                        upgrade_downloaded = true;

                        let version = upgrading_to.clone();
                        thread::spawn(move || {
                            notify::notify(
                                "distributor-logo-upgrade-symbolic",
                                &format!("Pop!_OS {} is ready to upgrade", version),
                                "Click here to restart",
                                || reboot(),
                            );
                        });

                        option_upgrade
                            .button_view()
                            .button_label("Upgrade")
                            .set_label(&format!("Pop!_OS {} download complete", &*upgrading_to));
                    }
                    UiEvent::Completed(CompletedEvent::Scan {
                        upgrade_text,
                        upgrade,
                        refresh,
                        is_lts,
                        status_failed,
                    }) => {
                        upgrade_version = upgrade;
                        refresh_found = refresh;

                        option_upgrade
                            .set_label(&upgrade_text)
                            .set_sublabel(None)
                            .set_button(if let Some(info) = upgrade_version.as_ref() {
                                upgrade_found = true;

                                if is_lts && !is_dismissed(&info.next) {
                                    let widget = {
                                        let sender = sender.clone();
                                        Dismisser::new(&info.next, move || {
                                            let _ =
                                                sender.send(BackgroundEvent::DismissNotification);
                                        })
                                    };

                                    dismisser_frame.foreach(WidgetExt::destroy);
                                    dismisser_frame.add(widget.as_ref());
                                    dismisser_frame.show_all();

                                    if let Some(dismisser) = dismisser.take() {
                                        dismisser.destroy();
                                    }

                                    dismisser = Some(widget);
                                }

                                let gui_sender = gui_sender.clone();
                                let action = move || {
                                    if let Some(sender) = gui_sender.upgrade() {
                                        let _ = sender.send(UiEvent::UpgradeClicked);
                                    }
                                };
                                Some(("Download", action))
                            } else {
                                None
                            })
                            .show_all();

                        if refresh {
                            let sender = sender.clone();
                            let action = move || {
                                let _ = sender.send(BackgroundEvent::RefreshOS);
                            };

                            option_refresh.set_button(Some(("Refresh", action))).show();
                        }

                        if status_failed {
                            option_upgrade.stack.hide();
                        }

                        container.show();
                    }
                    UiEvent::IncompatibleRepos(repos) => {
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
                            let _ = sender.send(BackgroundEvent::RepoModify(
                                failures,
                                dialog.answers().collect::<Vec<bool>>(),
                            ));
                        }

                        dialog.destroy();
                    }
                    // When the upgrade button is clicked, we will fetch the OS
                    UiEvent::UpgradeClicked => {
                        if upgrade_downloaded {
                            let dialog = UpgradeDialog::new(&upgrading_to, "Place changelog here");

                            if gtk::ResponseType::Accept == dialog.run() {
                                reboot()
                            } else {
                                return gtk::Continue(true);
                            }

                            dialog.destroy();
                        }

                        if let Some(info) = upgrade_version.clone() {
                            option_upgrade
                                .progress_label("Preparing to download")
                                .set_label("Updating your OS");

                            option_refresh.hide();

                            if let Some(dismisser) = dismisser.take() {
                                dismisser.destroy();
                            }

                            let _ = sender.send(BackgroundEvent::DownloadUpgrade(info));
                        }
                    }
                    UiEvent::StatusChanged(from, to, why) => {
                        warn!("status changed from {} to {}: {}", from, to, why);
                        let _ = sender.send(BackgroundEvent::GetStatus(from));
                    }
                    UiEvent::Error(why) => {
                        if refresh_found {
                            option_refresh.button_view().show_all();
                            get_refresh_row(&options).show();
                        }

                        if upgrade_found {
                            option_upgrade.button_view().show_all();
                            get_upgrade_row(&options).show();
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

                        error!("{}", error_message);
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

    pub fn shutdown(&self) { let _ = self.sender.send(BackgroundEvent::Shutdown); }

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
        send: impl Fn(UiEvent) + Send + Sync + 'static,
    ) {
        thread::spawn(move || {
            let send: &dyn Fn(UiEvent) = &send;
            if let Ok(ref client) = Client::new() {
                while let Ok(event) = receiver.recv() {
                    trace!("received BackgroundEvent: {:?}", event);
                    match event {
                        BackgroundEvent::GetStatus(from) => {
                            get_status(client, send, from);
                        }
                        BackgroundEvent::IsActive(tx) => {
                            let _ = tx.send(client.status().is_ok());
                        }
                        BackgroundEvent::DismissNotification => {
                            let event = match client.dismiss_notification() {
                                Ok(()) => UiEvent::Dismissed,
                                Err(why) => {
                                    UiEvent::Error(UiError::Dismiss(UnderlyingError::Client(why)))
                                }
                            };

                            send(event)
                        }
                        BackgroundEvent::RefreshOS => {
                            refresh_os(client, send);
                        }
                        BackgroundEvent::RepoModify(failures, answers) => {
                            repo_modify(client, send, failures, answers);
                        }
                        BackgroundEvent::Scan => scan(client, send),
                        BackgroundEvent::DownloadUpgrade(info) => {
                            download_upgrade(client, send, info);
                        }
                        BackgroundEvent::Shutdown => {
                            send(UiEvent::Shutdown);
                            debug!("stopping background thread");
                            break;
                        }
                    }
                }
            }

            debug!("breaking free");
        });
    }
}

fn scan(client: &Client, send: &dyn Fn(UiEvent)) {
    debug!("scanning");
    send(UiEvent::Initiated(InitiatedEvent::Scanning));
    let mut upgrade = None;
    let mut is_lts = false;
    let mut status_failed = false;

    let upgrade_text = if release::upgrade_in_progress() {
        Cow::Borrowed("Pop!_OS is currently downloading.")
    } else {
        match client.release_check() {
            Ok(info) => {
                is_lts = info.is_lts;
                eprintln!("info.build = {}", info.build);
                if info.build >= 0 {
                    info!("upgrade from {} to {} is available", info.current, info.next);

                    let upgrade_text = Cow::Owned(format!("Pop!_OS {} is available.", info.next));
                    upgrade = Some(info);
                    upgrade_text
                } else {
                    status_failed = true;
                    Cow::Borrowed(match info.build {
                        -1 => "Failed to retrieve build status due to an internal error.",
                        -2 => "You are running the most current Pop!_OS version.",
                        -3 => "Connection failed. You may be offline.",
                        _ => "Unknown status received.",
                    })
                }
            }
            Err(why) => {
                status_failed = true;
                error!("failed to check for updates: {}", why);
                Cow::Borrowed("Failed to check for updates")
            }
        }
    };

    send(UiEvent::Completed(CompletedEvent::Scan {
        upgrade_text: Box::from(upgrade_text.as_ref()),
        upgrade,
        refresh: client.recovery_exists(),
        is_lts,
        status_failed,
    }));
}

fn get_status(client: &Client, send: &dyn Fn(UiEvent), from: DaemonStatus) {
    match from {
        DaemonStatus::RecoveryUpgrade => {
            let event = match client.recovery_upgrade_release_status() {
                Ok(status) => {
                    if status.status == 0 {
                        UiEvent::Completed(CompletedEvent::Recovery)
                    } else {
                        UiEvent::Error(UiError::Recovery(status.why.into()))
                    }
                }
                Err(why) => UiEvent::Error(UiError::Recovery(why.into())),
            };

            send(event);
        }
        DaemonStatus::ReleaseUpgrade => {
            let event = match client.release_upgrade_status() {
                Ok(status) => {
                    if status.status == 0 {
                        UiEvent::Completed(CompletedEvent::Download)
                    } else {
                        UiEvent::Error(UiError::Upgrade(status.why.into()))
                    }
                }
                Err(why) => UiEvent::Error(UiError::Upgrade(why.into())),
            };

            send(event);
        }
        _ => (),
    }
}

fn refresh_os(client: &Client, send: &dyn Fn(UiEvent)) {
    send(UiEvent::Initiated(InitiatedEvent::Refresh));

    if let Err(why) = client.refresh_os(RefreshOp::Enable) {
        send(UiEvent::Error(UiError::Refresh(why.into())));
        return;
    }

    send(UiEvent::Completed(CompletedEvent::Refresh));
}

fn repo_modify(
    client: &Client,
    send: &dyn Fn(UiEvent),
    failures: Vec<Box<str>>,
    answers: Vec<bool>,
) {
    let input = failures.into_iter().zip(answers.into_iter());
    if let Err(why) = client.repo_modify(input) {
        send(UiEvent::Error(UiError::Repos(why.into())));
        return;
    }

    send(UiEvent::UpgradeClicked);
}

fn status_changed(send: &dyn Fn(UiEvent), new_status: Status, expected: DaemonStatus) {
    let status = DaemonStatus::from_u8(new_status.status).expect("unknown daemon status value");
    send(UiEvent::StatusChanged(expected, status, new_status.why));
}

fn update_system(client: &Client, send: &dyn Fn(UiEvent)) -> bool {
    info!("checking if updates are required");
    let updates = match client.fetch_updates(Vec::new(), false) {
        Ok(updates) => updates,
        Err(why) => {
            send(UiEvent::Error(UiError::Updates(why.into())));
            return false;
        }
    };

    if updates.updates_available {
        send(UiEvent::Progress(ProgressEvent::Fetching(
            updates.completed as u64,
            updates.total as u64,
        )));

        let error = &mut None;

        debug!("listening for package fetching signals");
        client.event_listen(
            DaemonStatus::FetchingPackages,
            Client::fetch_updates_status,
            |status| status_changed(send, status, DaemonStatus::FetchingPackages),
            |_client, signal| {
                match signal {
                    Signal::PackageFetchResult(status) => {
                        if status.status != 0 {
                            *error = Some(status.why);
                        }

                        return Ok(client::Continue(false));
                    }
                    Signal::PackageFetched(status) => {
                        send(UiEvent::Progress(ProgressEvent::Fetching(
                            status.completed as u64,
                            status.total as u64,
                        )));
                    }
                    Signal::PackageUpgrade(event) => {
                        if let Ok(AptUpgradeEvent::Progress { percent }) =
                            AptUpgradeEvent::from_dbus_map(event.into_iter())
                        {
                            send(UiEvent::Progress(ProgressEvent::Updates(percent)));
                        }
                    }
                    Signal::ReleaseEvent(event) => {
                        send(UiEvent::UpgradeEvent(event));
                    }
                    _ => (),
                }

                Ok(client::Continue(true))
            },
        );

        if let Some(why) = error.take() {
            send(UiEvent::Error(UiError::Updates(why.into())));
            return false;
        }
    }

    true
}

fn download_upgrade(client: &Client, send: &dyn Fn(UiEvent), info: ReleaseInfo) {
    info!("downloading updates for {}", info.next);
    if !update_system(client, send) {
        return;
    }

    let &ReleaseInfo { ref current, ref next, .. } = &info;
    // TODO: Re-enable this when QA is ready for testing this behavior.
    //    let how = if client.recovery_exists() {
    //        // Upgrade the recovery partition in addition to the OS.
    //        if !upgrade_recovery(client, send, next) {
    //            return;
    //        }
    //
    //        UpgradeMethod::Recovery
    //    } else {
    //        UpgradeMethod::Offline
    //    };

    let how = UpgradeMethod::Offline;

    send(UiEvent::Initiated(InitiatedEvent::Download(next.clone())));

    if let Err(why) = client.release_upgrade(how, current, next) {
        send(UiEvent::Error(UiError::Upgrade(why.into())));
        return;
    }

    let error = &mut None;
    let ignore_error = &mut false;
    let status_broken = &mut false;

    client.event_listen(
        DaemonStatus::ReleaseUpgrade,
        Client::release_upgrade_status,
        |status| {
            *status_broken = true;
            status_changed(send, status, DaemonStatus::ReleaseUpgrade);
        },
        |_client, signal| {
            match signal {
                Signal::PackageFetchResult(status) | Signal::RecoveryResult(status) => {
                    if status.status != 0 {
                        *error = Some(status.why);
                        return Ok(client::Continue(false));
                    }
                }
                Signal::PackageFetched(status) => {
                    send(UiEvent::Progress(ProgressEvent::Fetching(
                        status.completed as u64,
                        status.total as u64,
                    )));
                }
                Signal::PackageUpgrade(event) => {
                    if let Ok(AptUpgradeEvent::Progress { percent }) =
                        AptUpgradeEvent::from_dbus_map(event.into_iter())
                    {
                        send(UiEvent::Progress(ProgressEvent::Updates(percent)));
                    }
                }
                Signal::ReleaseEvent(event) => {
                    send(UiEvent::UpgradeEvent(event));
                }
                Signal::ReleaseResult(status) => {
                    if status.status != 0 {
                        *error = Some(status.why);
                    }

                    return Ok(client::Continue(false));
                }
                Signal::RecoveryDownloadProgress(progress) => {
                    send(UiEvent::Progress(ProgressEvent::Recovery(
                        progress.progress,
                        progress.total,
                    )));
                }
                Signal::RepoCompatError(repositories) => {
                    *ignore_error = true;
                    send(UiEvent::IncompatibleRepos(repositories));
                }
                _ => (),
            }

            Ok(client::Continue(true))
        },
    );

    if *ignore_error {
        return;
    }

    if let Some(why) = error.take() {
        send(UiEvent::Error(UiError::Upgrade(why.into())));
        return;
    }

    if !*status_broken {
        send(UiEvent::Completed(CompletedEvent::Download));
    }
}

fn upgrade_recovery(client: &Client, send: &dyn Fn(UiEvent), version: &str) -> bool {
    send(UiEvent::Initiated(InitiatedEvent::Recovery));

    let arch = "nvidia";
    let flags = ReleaseFlags::empty();

    if let Err(why) = client.recovery_upgrade_release(version, arch, flags) {
        send(UiEvent::Error(UiError::Recovery(why.into())));
        return false;
    }

    let error = &mut None;

    client.event_listen(
        DaemonStatus::RecoveryUpgrade,
        Client::recovery_upgrade_release_status,
        |status| status_changed(send, status, DaemonStatus::RecoveryUpgrade),
        |_client, signal| {
            match signal {
                Signal::RecoveryDownloadProgress(progress) => {
                    send(UiEvent::Progress(ProgressEvent::Recovery(
                        progress.progress,
                        progress.total,
                    )));
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
        send(UiEvent::Error(UiError::Recovery(why.into())));
        return false;
    }

    send(UiEvent::Completed(CompletedEvent::Recovery));
    true
}

fn reboot() { let _ = Command::new("systemctl").arg("reboot").status(); }

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
