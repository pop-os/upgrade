pub mod methods;
pub mod signals;

mod dbus_helper;
mod error;
mod runtime;
mod status;

pub use self::{
    dbus_helper::DbusFactory, error::DaemonError, methods::DismissEvent, runtime::DaemonRuntime,
    signals::SignalEvent, status::DaemonStatus,
};

use crate::{
    misc::{self, format_error},
    recovery::{
        self, RecoveryError, RecoveryVersion, RecoveryVersionError,
        ReleaseFlags as RecoveryReleaseFlags, UpgradeMethod as RecoveryUpgradeMethod,
    },
    release::{
        self, FetchEvent, RefreshOp, ReleaseError, ReleaseStatus,
        UpgradeMethod as ReleaseUpgradeMethod,
    },
    sighandler, DBUS_IFACE, DBUS_NAME, DBUS_PATH, RESTART_SCHEDULED,
};

use anyhow::Context;
use apt_cmd::{request::Request as AptRequest, AptCache, AptGet, AptMark};
use as_result::*;
use atomic::Atomic;
use dbus::{
    self,
    tree::{Factory, Signal},
    BusType, Connection, Message, NameFlag,
};
use flume::{bounded, Receiver, Sender};
use futures::prelude::*;
use logind_dbus::LoginManager;
use num_traits::FromPrimitive;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

pub const DISMISSED: &str = "/usr/lib/pop-upgrade/dismissed";
pub const INSTALL_DATE: &str = "/usr/lib/pop-upgrade/install_date";

#[derive(Debug)]
pub enum Event {
    Cancel,
    FetchUpdates { apt_uris: HashSet<AptRequest>, download_only: bool },
    PackageUpgrade,
    RecoveryUpgrade(RecoveryUpgradeMethod),
    ReleaseUpgrade { how: ReleaseUpgradeMethod, from: String, to: String },
}

#[derive(Debug)]
pub enum FgEvent {
    SetUpgradeState(Result<(), ReleaseError>, ReleaseUpgradeMethod, Box<str>, Box<str>),
}

pub struct LastKnown {
    fetch: Result<(), ReleaseError>,
    recovery_upgrade: Result<(), RecoveryError>,
    release_upgrade: Result<(), ReleaseError>,
}

impl Default for LastKnown {
    fn default() -> Self {
        Self { fetch: Ok(()), recovery_upgrade: Ok(()), release_upgrade: Ok(()) }
    }
}

pub struct ReleaseUpgradeState {
    action: release::UpgradeMethod,
    from: Box<str>,
    to: Box<str>,
}

pub struct Daemon {
    event_tx: Sender<Event>,
    fg_rx: Receiver<FgEvent>,
    dbus_rx: Receiver<SignalEvent>,
    connection: Arc<Connection>,
    status: Arc<Atomic<DaemonStatus>>,
    sub_status: Arc<Atomic<u8>>,
    fetching_state: Arc<Atomic<(u64, u64)>>,
    cancel: Arc<AtomicBool>,
    last_known: LastKnown,
    release_upgrade: Option<ReleaseUpgradeState>,
    perform_upgrade: bool,
}

impl Daemon {
    pub fn new(_factory: &DbusFactory) -> Result<Self, DaemonError> {
        let connection = Arc::new(
            Connection::get_private(BusType::System).map_err(DaemonError::PrivateConnection)?,
        );

        connection
            .register_name(DBUS_NAME, NameFlag::ReplaceExisting as u32)
            .map_err(DaemonError::RegisterName)?;

        // Only accept one event at a time.
        let (event_tx, event_rx) = bounded(4);

        // Events to be handled in the foreground.
        let (fg_tx, fg_rx) = bounded(4);

        // Dbus events are checked at least once per second, so we will allow buffering some events.
        let (dbus_tx, dbus_rx) = bounded(64);

        // The status of the event loop thread, which indicates the current task, or lack thereof.
        let status = Arc::new(Atomic::new(DaemonStatus::Inactive));
        // As well as the current sub-status, if relevant.
        let sub_status = Arc::new(Atomic::new(0u8));

        // In case a UI is being constructed after a task has already started, it may request
        // for the curernt progress of a task.
        let prog_state = Arc::new(Atomic::new((0u64, 0u64)));

        // Cancels a process which is in progress
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_process: Arc<dyn Fn() -> bool + Send + Sync> =
            Arc::new(enclose!((cancel => c) move || c.swap(false, Ordering::SeqCst)));

        let mut processing = false;

        std::thread::spawn(
            enclose!((cancel, status, sub_status, prog_state) move || async_io::block_on(async move {
                let mut logind = match LoginManager::new() {
                    Ok(logind) => Some(logind),
                    Err(why) => {
                        error!("failed to connect to logind: {}", why);
                        None
                    }
                };

                let mut runtime = DaemonRuntime::new();

                let fetch_closure = Arc::new(enclose!((prog_state, dbus_tx) move |event| {
                    match event {
                        FetchEvent::Fetched(uri) => {
                            let (current, npackages) = prog_state.load(Ordering::SeqCst);
                            prog_state.store((current + 1, npackages), Ordering::SeqCst);

                            let _ = dbus_tx.send(SignalEvent::Fetched(
                                uri.name,
                                current as u32 + 1,
                                npackages as u32,
                            ));
                        }
                        FetchEvent::Fetching(uri) => {
                            let _ = dbus_tx.send(SignalEvent::Fetching(uri.name));
                        }
                        FetchEvent::Init(total) => {
                            prog_state.store((0, total as u64), Ordering::SeqCst);
                        }
                    }
                }));

                while let Ok(event) = event_rx.recv() {
                    let _suspend_lock = logind.as_mut().and_then(|logind| {
                        match logind
                            .connect()
                            .inhibit_suspend("pop-upgrade", "performing upgrade event")
                        {
                            Ok(lock) => Some(lock),
                            Err(why) => {
                                error!("failed to inhibit suspension: {}", why);
                                None
                            }
                        }
                    });

                    match event {
                        Event::Cancel => {
                            if processing {
                                cancel.store(true, Ordering::SeqCst);
                                continue;
                            }
                        }

                        Event::FetchUpdates { apt_uris, download_only } => {
                            info!("fetching packages for {:?}", apt_uris);
                            let npackages = apt_uris.len() as u32;
                            prog_state.store((0, u64::from(npackages)), Ordering::SeqCst);

                            let result = runtime.apt_fetch(apt_uris, fetch_closure.clone()).await;
                            info!("fetched");


                            prog_state.store((0, 0), Ordering::SeqCst);

                            let result = match result {
                                Ok(_) => {
                                    if download_only {
                                        Ok(())
                                    } else {
                                        (async {
                                            info!("performing upgrade");
                                            let (mut child, events) = AptGet::new()
                                                .noninteractive()
                                                .allow_downgrades()
                                                .force()
                                                .stream_upgrade()
                                                .await
                                                .map_err(ReleaseError::Upgrade)?;

                                            futures_util::pin_mut!(events);

                                            while let Some(event) = events.next().await {
                                                let _ = dbus_tx.send(SignalEvent::Upgrade(event));
                                            }

                                            info!("completed apt upgrade");

                                            child.status().await.map_result().map_err(ReleaseError::Upgrade)
                                        }).await
                                    }
                                }
                                Err(why) => Err(why)
                            };

                            let _ = dbus_tx.send(SignalEvent::FetchResult(result));
                        }

                        Event::PackageUpgrade => {
                            info!("upgrading packages");
                            let _ = runtime.package_upgrade(|event| {
                                let _ = dbus_tx.send(SignalEvent::Upgrade(event));
                            });
                        }

                        Event::RecoveryUpgrade(action) => {
                            processing = true;
                            info!("attempting recovery upgrade with {:?}", action);
                            let result = recovery::recovery(
                                &|| (*cancel_process)(),
                                &action,
                                enclose!((dbus_tx, prog_state) move |p, t| {
                                    prog_state.store((p, t), Ordering::SeqCst);
                                    let _ = dbus_tx
                                        .send(SignalEvent::RecoveryDownloadProgress(p, t));
                                }),
                                enclose!((dbus_tx, sub_status) move |status| {
                                    sub_status.store(status as u8, Ordering::SeqCst);
                                    let _ =
                                        dbus_tx.send(SignalEvent::RecoveryUpgradeEvent(status));
                                }),
                            ).await;

                            let _ = dbus_tx.send(SignalEvent::RecoveryUpgradeResult(result));
                            processing = false;
                        }

                        Event::ReleaseUpgrade { how, from, to } => {
                            info!(
                                "attempting release upgrade, using a {}",
                                <&'static str>::from(how)
                            );

                            let progress = enclose!((dbus_tx, sub_status) move |event| {
                                let _ = dbus_tx.send(SignalEvent::ReleaseUpgradeEvent(event));
                                sub_status.store(event as u8, Ordering::SeqCst);
                            });

                            let result = runtime.upgrade(
                                how,
                                &from,
                                &to,
                                &progress,
                                fetch_closure.clone(),
                                &|event| {
                                    let _ = dbus_tx.send(SignalEvent::Upgrade(event));
                                },
                            ).await;

                            let _ = AptMark::new().unhold(&["pop-upgrade"]).await;

                            let _ = fg_tx.send(FgEvent::SetUpgradeState(
                                result,
                                how,
                                from.into(),
                                to.into(),
                            ));
                        }
                    }

                    cancel.store(false, Ordering::SeqCst);
                    status.store(DaemonStatus::Inactive, Ordering::SeqCst);
                    info!("event processed");
                }
            })),
        );

        Ok(Daemon {
            cancel,
            connection,
            dbus_rx,
            event_tx,
            fetching_state: prog_state,
            fg_rx,
            last_known: Default::default(),
            release_upgrade: None,
            status,
            sub_status,
            perform_upgrade: false,
        })
    }

    pub fn init() -> Result<(), DaemonError> {
        info!("initializing daemon");
        fs::create_dir_all(crate::VAR_LIB_DIR)
            .map_err(|why| DaemonError::VarLibDirectory(crate::VAR_LIB_DIR, why))?;

        if let Err(why) = release::systemd::restore_default() {
            warn!("failure restoring previous boot entry: {}", why);
        }

        let factory = Factory::new_fn::<()>();

        let dbus_factory = DbusFactory::new(&factory);
        let daemon = Rc::new(RefCell::new(Self::new(&dbus_factory)?));

        let fetch_result = Arc::new(
            dbus_factory
                .signal(signals::PACKAGE_FETCH_RESULT)
                .sarg::<u8>("status")
                .sarg::<&str>("why")
                .consume(),
        );

        let fetching_package = Arc::new(
            dbus_factory.signal(signals::PACKAGE_FETCHING).sarg::<&str>("package").consume(),
        );

        let fetched_package = Arc::new(
            dbus_factory
                .signal(signals::PACKAGE_FETCHED)
                .sarg::<&str>("package")
                .sarg::<u32>("completed")
                .sarg::<u32>("total")
                .consume(),
        );

        let no_connection = Arc::new(dbus_factory.signal(signals::NO_CONNECTION).consume());

        let recovery_download_progress = Arc::new(
            dbus_factory
                .signal(signals::RECOVERY_DOWNLOAD_PROGRESS)
                .sarg::<u64>("current")
                .sarg::<u64>("total")
                .consume(),
        );

        let recovery_event =
            Arc::new(dbus_factory.signal(signals::RECOVERY_EVENT).sarg::<u8>("event").consume());

        let recovery_result = Arc::new(
            dbus_factory
                .signal(signals::RECOVERY_RESULT)
                .sarg::<u8>("result")
                .sarg::<&str>("why")
                .consume(),
        );

        let release_event =
            Arc::new(dbus_factory.signal(signals::RELEASE_EVENT).sarg::<u8>("event").consume());

        let release_result = Arc::new(
            dbus_factory
                .signal(signals::RELEASE_RESULT)
                .sarg::<u8>("result")
                .sarg::<&str>("why")
                .consume(),
        );

        let repo_compat_error = Arc::new(
            dbus_factory
                .signal(signals::REPO_COMPAT_ERROR)
                .sarg::<&[&str]>("success")
                .sarg::<&[(&str, &str)]>("failed")
                .consume(),
        );

        let upgrade_event = Arc::new(
            dbus_factory
                .signal(signals::PACKAGE_UPGRADE)
                .sarg::<HashMap<&str, String>>("event")
                .consume(),
        );

        let interface = factory
            .interface(DBUS_IFACE, ())
            .add_m(methods::cancel(daemon.clone(), &dbus_factory))
            .add_m(methods::dismiss_notification(daemon.clone(), &dbus_factory))
            .add_m(methods::fetch_updates_status(daemon.clone(), &dbus_factory))
            .add_m(methods::fetch_updates(daemon.clone(), &dbus_factory))
            .add_m(methods::package_upgrade(daemon.clone(), &dbus_factory))
            .add_m(methods::recovery_upgrade_file(daemon.clone(), &dbus_factory))
            .add_m(methods::recovery_upgrade_release(daemon.clone(), &dbus_factory))
            .add_m(methods::recovery_upgrade_status(daemon.clone(), &dbus_factory))
            .add_m(methods::recovery_version(daemon.clone(), &dbus_factory))
            .add_m(methods::refresh_os(daemon.clone(), &dbus_factory))
            .add_m(methods::release_check(daemon.clone(), &dbus_factory))
            .add_m(methods::release_repair(daemon.clone(), &dbus_factory))
            .add_m(methods::release_upgrade(daemon.clone(), &dbus_factory))
            .add_m(methods::release_upgrade_finalize(daemon.clone(), &dbus_factory))
            .add_m(methods::release_upgrade_status(daemon.clone(), &dbus_factory))
            .add_m(methods::reset(daemon.clone(), &dbus_factory))
            .add_m(methods::status(daemon.clone(), &dbus_factory))
            .add_m(methods::update_check(daemon.clone(), &dbus_factory))
            .add_s(fetch_result.clone())
            .add_s(fetched_package.clone())
            .add_s(fetching_package.clone())
            .add_s(no_connection.clone())
            .add_s(recovery_download_progress.clone())
            .add_s(recovery_event.clone())
            .add_s(recovery_result.clone())
            .add_s(release_result)
            .add_s(repo_compat_error)
            .add_s(upgrade_event.clone());

        let (connection, fg_receiver, receiver) = {
            let daemon = daemon.borrow();
            (daemon.connection.clone(), daemon.fg_rx.clone(), daemon.dbus_rx.clone())
        };

        let tree = factory
            .tree(())
            .add(factory.object_path(DBUS_PATH, ()).introspectable().add(interface));

        tree.set_registered(&connection, true).map_err(DaemonError::TreeRegister)?;

        connection.add_handler(tree);

        info!("daemon registered -- listening for new events");

        async_io::block_on(async move {
            release::cleanup().await;

            loop {
                connection.incoming(1000).next();

                if daemon.borrow().perform_upgrade {
                    let mut packages = vec!["pop-upgrade", "libpop-upgrade-gtk"];

                    if let Ok((_, mut policies)) =
                        AptCache::new().policy(&["libpop-upgrade-gtk-dev"]).await
                    {
                        if let Some(policy) = policies.next().await {
                            if policy.installed != "(none)" {
                                packages.push("libpop-upgrade-gtk-dev")
                            }
                        }
                    }

                    self_upgrade(&packages).await;
                }

                if let Some(status) = sighandler::status() {
                    info!("received a '{}' signal", status);

                    use sighandler::Signal::*;

                    match status {
                        Terminate => {
                            info!("terminating daemon");
                            break Ok(());
                        }
                        TermStop => {
                            info!("stopping daemon");
                            break Ok(());
                        }
                        _ => (),
                    }
                }

                while let Ok(fg_event) = fg_receiver.try_recv() {
                    match fg_event {
                        FgEvent::SetUpgradeState(result, action, from, to) => {
                            if result.is_ok() {
                                info!("setting release upgrade state");
                                let state = ReleaseUpgradeState { action, from, to };
                                daemon.borrow_mut().release_upgrade = Some(state);
                            }

                            daemon.borrow_mut().last_known.release_upgrade = result;
                        }
                    }
                }

                while let Ok(dbus_event) = receiver.try_recv() {
                    Self::send_signal_message(&connection, {
                        match &dbus_event {
                            SignalEvent::Fetched(..)
                            | SignalEvent::Fetching(_)
                            | SignalEvent::RecoveryUpgradeEvent(_)
                            | SignalEvent::RecoveryUpgradeResult(_)
                            | SignalEvent::ReleaseUpgradeEvent(_)
                            | SignalEvent::Upgrade(_) => info!("{}", dbus_event),
                            _ => (),
                        }

                        match dbus_event {
                            SignalEvent::FetchResult(result) => {
                                let (status, why) = result_signal(result.as_ref());
                                let message =
                                    Self::signal_message(&fetch_result).append2(status, why);

                                (*daemon.borrow_mut()).last_known.fetch = result;
                                message
                            }
                            SignalEvent::Fetched(name, completed, total) => Self::signal_message(
                                &fetched_package,
                            )
                            .append3(name.as_str(), completed, total),
                            SignalEvent::Fetching(name) => {
                                Self::signal_message(&fetching_package).append1(name.as_str())
                            }
                            SignalEvent::NoConnection => Self::signal_message(&no_connection),
                            SignalEvent::RecoveryDownloadProgress(progress, total) => {
                                Self::signal_message(&recovery_download_progress)
                                    .append2(progress, total)
                            }
                            SignalEvent::RecoveryUpgradeEvent(event) => {
                                Self::signal_message(&recovery_event).append1(event as u8)
                            }
                            SignalEvent::RecoveryUpgradeResult(result) => {
                                let (status, why) = result_signal(result.as_ref());
                                let message =
                                    Self::signal_message(&recovery_result).append2(status, why);

                                (*daemon.borrow_mut()).last_known.recovery_upgrade = result;
                                message
                            }
                            SignalEvent::ReleaseUpgradeEvent(event) => {
                                Self::signal_message(&release_event).append1(event as u8)
                            }
                            SignalEvent::Upgrade(ref event) => Self::signal_message(&upgrade_event)
                                .append1(event.clone().into_dbus_map()),
                        }
                    })
                }
            }
        })
    }

    /// Dismiss future desktop notifications.
    ///
    /// Only applicable for LTS releases.
    fn dismiss_notification(&self, event: DismissEvent) -> Result<bool, String> {
        if let DismissEvent::Unset = event {
            dismiss_file_remove()?;
            Ok(false)
        } else {
            let status = self.release_check(false)?;
            if status.is_lts() && status.build.is_ok() {
                dismiss_file_create(&status.next)?;

                if let DismissEvent::ByTimestamp = event {
                    crate::install::time()
                        .map_err(|why| format!("install timestamp: {}", why))
                        .and_then(dismissed_by_timestamp)?;
                }
            }

            Ok(true)
        }
    }

    async fn fetch_updates<'a>(
        &'a mut self,
        additional_packages: &'a [String],
        download_only: bool,
    ) -> anyhow::Result<(bool, u32)> {
        info!("fetching updates for the system, including {:?}", additional_packages);

        let mut borrows = Vec::with_capacity(additional_packages.len());
        borrows.extend(additional_packages.into_iter().map(String::as_str));

        let apt_uris = crate::fetch::apt::fetch_uris(Some(&borrows)).await?;

        if apt_uris.is_empty() {
            info!("no updates available to fetch");
            return Ok((false, 0));
        }

        let npackages = apt_uris.len() as u32;
        let event = Event::FetchUpdates { apt_uris, download_only };
        self.submit_event(event)?;

        Ok((true, npackages))
    }

    fn package_upgrade(&mut self) -> anyhow::Result<()> {
        info!("upgrading packages for the release");

        self.submit_event(Event::PackageUpgrade)?;
        Ok(())
    }

    fn cancel(&mut self) {
        info!("cancelling a process which is in progress");

        self.cancel.store(true, Ordering::SeqCst);
    }

    fn recovery_upgrade_file(&mut self, path: &str) -> anyhow::Result<()> {
        info!("using {} to upgrade the recovery partition", path);

        let event = Event::RecoveryUpgrade(RecoveryUpgradeMethod::FromFile(PathBuf::from(path)));

        self.submit_event(event)
    }

    fn recovery_upgrade_release(
        &mut self,
        version: &str,
        arch: &str,
        flags: u8,
    ) -> anyhow::Result<()> {
        info!("upgrading the recovery partition to {}-{}", version, arch);

        let event = Event::RecoveryUpgrade(RecoveryUpgradeMethod::FromRelease {
            version: if version.is_empty() { None } else { Some(version.into()) },
            arch: if arch.is_empty() { None } else { Some(arch.into()) },
            flags: RecoveryReleaseFlags::from_bits_truncate(flags),
        });

        self.submit_event(event)
    }

    fn recovery_version(&mut self) -> Result<RecoveryVersion, String> {
        info!("checking recovery version");

        let version = match crate::recovery::version() {
            Ok(version) => version,
            Err(RecoveryVersionError::Unknown) => {
                RecoveryVersion { version: String::new(), build: -1 }
            }
            Err(ref why) => {
                return Err(format_error(why))?;
            }
        };

        Ok(version)
    }

    fn refresh_os(&mut self, flag: RefreshOp) -> Result<bool, String> {
        info!("preparing to refresh OS");
        crate::release::refresh_os(flag).map_err(|ref why| format_error(why))
    }

    fn release_check(&self, development: bool) -> Result<ReleaseStatus, String> {
        info!("performing a release check");

        let status = release::check::next(development).map_err(|ref why| format_error(why))?;

        let mut buffer = String::new();

        info!(
            "Release {{ current: \"{}\", lts: \"{}\",  next: \"{}\", available: {} }}",
            status.current,
            status.is_lts(),
            status.next,
            misc::format_build_number(status.build.status_code(), &mut buffer)
        );

        Ok(status)
    }

    fn release_upgrade(&mut self, how: u8, from: &str, to: &str) -> anyhow::Result<()> {
        info!("upgrading release from {} to {}, with {}", from, to, how);

        let how = ReleaseUpgradeMethod::from_u8(how)
            .context("provided upgrade `how` value is out of range")?;

        let event = Event::ReleaseUpgrade { how, from: from.into(), to: to.into() };
        self.submit_event(event)
    }

    fn release_upgrade_finalize(&mut self) -> Result<(), String> {
        match self.release_upgrade.as_ref() {
            Some(ReleaseUpgradeState { action, from, to }) => {
                release::upgrade_finalize(*action, from, to)
                    .map_err(|why| format!("release upgrade finalization failed: {}", why))
            }
            None => Err("release upgrade cannot be finalized, because a release upgrade was not \
                         performed"
                .into()),
        }
    }

    async fn release_repair(&mut self) -> anyhow::Result<()> {
        crate::repair::repair().await?;

        Ok(())
    }

    async fn reset(&mut self) -> Result<(), String> {
        info!("resetting daemon");

        self.status.store(DaemonStatus::Inactive, Ordering::SeqCst);
        self.sub_status.store(0, Ordering::SeqCst);
        self.fetching_state.store((0, 0), Ordering::SeqCst);
        self.release_upgrade = None;

        release::cleanup().await;

        Ok(())
    }

    fn send_signal_message(connection: &Connection, message: Message) {
        if let Err(()) = connection.send(message) {
            error!("failed to send dbus signal message");
        }
    }

    fn set_status<T, E, F>(&mut self, status: DaemonStatus, mut func: F) -> Result<T, E>
    where
        F: FnMut(&mut Self, bool) -> Result<T, E>,
    {
        let already_active = self.status.swap(status, Ordering::SeqCst) == status;
        match func(self, already_active) {
            Ok(value) => Ok(value),
            Err(why) => {
                self.status.store(DaemonStatus::Inactive, Ordering::SeqCst);
                Err(why)
            }
        }
    }

    fn signal_message(signal: &Arc<Signal<()>>) -> Message {
        signal.msg(&DBUS_PATH.into(), &DBUS_NAME.into())
    }

    fn submit_event(&self, event: Event) -> anyhow::Result<()> {
        let desc = "too many requests sent -- refusing additional requests";

        if self.event_tx.is_full() {
            warn!("{}", desc);
            return Err(anyhow::anyhow!("{}", desc));
        }

        let _ = self.event_tx.send(event);
        Ok(())
    }

    async fn update_and_restart(&mut self) -> u8 {
        info!("updating apt sources");
        let _ = AptGet::new().update().await;

        if let Ok(true) = upgrade_required().await {
            if async_fs::File::create(RESTART_SCHEDULED).await.is_ok() {
                info!("installing latest version of `pop-upgrade`, which will restart the daemon");
                self.perform_upgrade = true;
                return 1;
            }
        }

        0
    }
}

pub async fn upgrade_required() -> anyhow::Result<bool> {
    let (_, mut policies) = apt_cmd::AptCache::new().policy(&["pop-upgrade"]).await?;

    if let Some(policy) = policies.next().await {
        if policy.installed != policy.candidate {
            return Ok(true);
        }
    }

    Ok(false)
}

pub fn result_signal<E: ::std::fmt::Display>(result: Result<&(), &E>) -> (u8, String) {
    let status = match result {
        Ok(_) => 0u8,
        Err(_) => 1,
    };

    let why: String = result.err().map(|why| fomat!((why))).unwrap_or_default();

    (status, why)
}

// Creates the notification dismissal file.
fn dismiss_file_create(next: &str) -> Result<(), String> {
    fs::write(DISMISSED, next.as_bytes())
        .map_err(|why| format!("failed to write '{}' to '{}': {}", next, DISMISSED, why))
}

/// Removes the notification dismissal file.
fn dismiss_file_remove() -> Result<(), String> {
    fs::remove_file(DISMISSED).map_err(|why| format!("failed to remove '{}': {}", DISMISSED, why))
}

/// Creates the file which is used by clients to know that a release was dismissed by timestamp.
///
/// This file contains the timestamp of the install date.
fn dismissed_by_timestamp(timestamp: i64) -> Result<(), String> {
    fs::write(INSTALL_DATE, timestamp.to_string().as_bytes())
        .map_err(|why| format!("install timestamp write: {}", why))
}

/// Installs packages in background, ensuring that the process continues
/// even if the daemon is restarted
async fn self_upgrade(packages: &[&str]) {
    let _ = AptGet::new().noninteractive().fix_broken().allow_downgrades().force().status().await;
    let _ = AptGet::new().allow_downgrades().force().install(packages).await;

    std::process::exit(1);
}
