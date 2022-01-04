pub mod signals;

pub mod methods {
    #[repr(u8)]
    #[derive(Clone, Copy, Debug, FromPrimitive, PartialEq)]
    pub enum DismissEvent {
        ByTimestamp = 1,
        ByUser = 2,
        Unset = 3,
    }

    pub const CANCEL: &str = "Cancel";
    pub const DISMISS_NOTIFICATION: &str = "DismissNotification";
    pub const FETCH_UPDATES: &str = "FetchUpdates";
    pub const FETCH_UPDATES_STATUS: &str = "FetchUpdatesStatus";
    pub const PACKAGE_UPGRADE: &str = "UpgradePackages";
    pub const RECOVERY_UPGRADE_FILE: &str = "RecoveryUpgradeFile";
    pub const RECOVERY_UPGRADE_RELEASE: &str = "RecoveryUpgradeRelease";
    pub const RECOVERY_UPGRADE_RELEASE_STATUS: &str = "RecoveryUpgradeReleaseStatus";
    pub const RECOVERY_VERSION: &str = "RecoveryVersion";
    pub const REFRESH_OS: &str = "RefreshOS";
    pub const RELEASE_CHECK: &str = "ReleaseCheck";
    pub const RELEASE_UPGRADE: &str = "ReleaseUpgrade";
    pub const RELEASE_UPGRADE_FINALIZE: &str = "ReleaseUpgradeFinalize";
    pub const RELEASE_UPGRADE_STATUS: &str = "ReleaseUpgradeStatus";
    pub const RELEASE_REPAIR: &str = "ReleaseRepair";
    pub const RESET: &str = "Reset";
    pub const STATUS: &str = "Status";
    pub const UPDATE_CHECK: &str = "UpdateCheck";
}

mod error;
mod status;

pub use self::{
    error::DaemonError, methods::DismissEvent, signals::SignalEvent, status::DaemonStatus,
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

use anyhow::Context as AnyhowContext;
use apt_cmd::{request::Request as AptRequest, AptCache, AptGet, AptMark};
use as_result::MapResult;
use atomic::Atomic;
use dbus::{
    blocking::Connection,
    channel::{MatchingReceiver, Sender as DBusSender},
    message::{MatchRule, Message},
};
use dbus_crossroads::{Context, Crossroads, MethodErr};
use flume::{bounded, Receiver, Sender};
use futures_util::StreamExt;
use logind_dbus::LoginManager;
use num_traits::FromPrimitive;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc,
    },
};

pub const DISMISSED: &str = "/usr/lib/pop-upgrade/dismissed";
pub const INSTALL_DATE: &str = "/usr/lib/pop-upgrade/install_date";

#[derive(Debug)]
pub enum Event {
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
    fetch:            Result<(), ReleaseError>,
    recovery_upgrade: Result<(), RecoveryError>,
    release_upgrade:  Result<(), ReleaseError>,
}

impl Default for LastKnown {
    fn default() -> Self {
        Self { fetch: Ok(()), recovery_upgrade: Ok(()), release_upgrade: Ok(()) }
    }
}

pub struct ReleaseUpgradeState {
    action: release::UpgradeMethod,
    from:   Box<str>,
    to:     Box<str>,
}

struct SharedState {
    // Cancels a process which is in progress
    cancel:         AtomicBool,
    // In case a UI is being constructed after a task has already started, it may request
    // for the curernt progress of a task.
    fetching_state: Atomic<(u64, u64)>,
    // The status of the event loop thread, which indicates the current task, or lack thereof.
    status:         Atomic<DaemonStatus>,
    // As well as the current sub-status, if relevant.
    sub_status:     AtomicU8,
}

pub struct Daemon {
    dbus_rx:         Receiver<SignalEvent>,
    event_tx:        Sender<Event>,
    fg_rx:           Receiver<FgEvent>,
    last_known:      LastKnown,
    perform_upgrade: bool,
    release_upgrade: Option<ReleaseUpgradeState>,
    shared_state:    Arc<SharedState>,
}

impl Daemon {
    pub fn new() -> Result<Self, DaemonError> {
        // Only accept one event at a time.
        let (event_tx, event_rx) = bounded(4);

        // Events to be handled in the foreground.
        let (fg_tx, fg_rx) = bounded(4);

        // Dbus events are checked at least once per second, so we will allow buffering some events.
        let (dbus_tx, dbus_rx) = bounded(64);

        // State shared between the background task thread, and the foreground DBus event loop.
        let shared_state = Arc::new(SharedState {
            status:         Atomic::new(DaemonStatus::Inactive),
            sub_status:     AtomicU8::new(0),
            fetching_state: Atomic::new((0, 0)),
            cancel:         AtomicBool::new(false),
        });

        let cancel_process =
            enclose!((shared_state) move || shared_state.cancel.swap(false, Ordering::SeqCst));

        std::thread::spawn(enclose!((shared_state) move || async_io::block_on(async move {
            let mut logind = match LoginManager::new() {
                Ok(logind) => Some(logind),
                Err(why) => {
                    error!("failed to connect to logind: {}", why);
                    None
                }
            };

            let fetch_closure = enclose!((dbus_tx, shared_state) move |event| {
                match event {
                    FetchEvent::Fetched(uri) => {
                        let (current, npackages) = shared_state.fetching_state.load(Ordering::SeqCst);
                        shared_state.fetching_state.store((current + 1, npackages), Ordering::SeqCst);

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
                        shared_state.fetching_state.store((0, total as u64), Ordering::SeqCst);
                    }
                }
            });

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
                    Event::FetchUpdates { apt_uris, download_only } => {
                        info!("fetching packages for {:?}", apt_uris);
                        let npackages = apt_uris.len() as u32;
                        shared_state.fetching_state.store((0, u64::from(npackages)), Ordering::SeqCst);

                        let result = crate::release::apt_fetch(apt_uris, &fetch_closure).await;
                        info!("fetched");

                        shared_state.fetching_state.store((0, 0), Ordering::SeqCst);

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
                        let _ = crate::release::package_upgrade(|event| {
                            let _ = dbus_tx.send(SignalEvent::Upgrade(event));
                        });
                    }

                    Event::RecoveryUpgrade(action) => {
                        info!("attempting recovery upgrade with {:?}", action);
                        let result = recovery::recovery(
                            &cancel_process,
                            &action,
                            enclose!((dbus_tx, shared_state) move |p, t| {
                                shared_state.fetching_state.store((p, t), Ordering::SeqCst);
                                let _ = dbus_tx
                                    .send(SignalEvent::RecoveryDownloadProgress(p, t));
                            }),
                            enclose!((dbus_tx, shared_state) move |status| {
                                shared_state.sub_status.store(status as u8, Ordering::SeqCst);
                                let _ =
                                    dbus_tx.send(SignalEvent::RecoveryUpgradeEvent(status));
                            }),
                        ).await;

                        let _ = dbus_tx.send(SignalEvent::RecoveryUpgradeResult(result));
                    }

                    Event::ReleaseUpgrade { how, from, to } => {
                        shared_state.status.store(DaemonStatus::ReleaseUpgrade, Ordering::SeqCst);

                        info!(
                            "attempting release upgrade, using a {}",
                            <&'static str>::from(how)
                        );

                        let progress = enclose!((dbus_tx, shared_state) move |event| {
                            let _ = dbus_tx.send(SignalEvent::ReleaseUpgradeEvent(event));
                            shared_state.sub_status.store(event as u8, Ordering::SeqCst);
                        });

                        let result = crate::release::upgrade(
                            how,
                            &from,
                            &to,
                            &progress,
                            &fetch_closure,
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

                shared_state.cancel.store(false, Ordering::SeqCst);
                shared_state.status.store(DaemonStatus::Inactive, Ordering::SeqCst);
                info!("event processed");
            }
        })));

        Ok(Daemon {
            dbus_rx,
            event_tx,
            fg_rx,
            last_known: Default::default(),
            release_upgrade: None,
            perform_upgrade: false,
            shared_state,
        })
    }

    pub fn init() -> Result<(), DaemonError> {
        info!("initializing daemon");
        fs::create_dir_all(crate::VAR_LIB_DIR)
            .map_err(|why| DaemonError::VarLibDirectory(crate::VAR_LIB_DIR, why))?;

        if let Err(why) = release::systemd::restore_default() {
            warn!("failure restoring previous boot entry: {}", why);
        }

        let daemon = Self::new()?;

        let connection = Connection::new_system().map_err(DaemonError::PrivateConnection)?;

        connection
            .request_name(DBUS_NAME, false, true, false)
            .map_err(DaemonError::RegisterName)?;

        let mut cr = Crossroads::new();

        let iface_token = cr.register(DBUS_IFACE, |b| {
            let _fetch_result =
                b.signal::<(u8, String), _>(signals::PACKAGE_FETCH_RESULT, ("status", "why"));

            let _fetching_package =
                b.signal::<(String,), _>(signals::PACKAGE_FETCHING, ("package",));

            let _fetched_package = b.signal::<(String, u32, u32), _>(
                signals::PACKAGE_FETCHED,
                ("package", "completed", "total"),
            );

            let _no_connection = b.signal::<(), _>(signals::NO_CONNECTION, ());

            let _recovery_download_progress = b
                .signal::<(u64, u64), _>(signals::RECOVERY_DOWNLOAD_PROGRESS, ("current", "total"));

            let _recovery_event = b.signal::<(u8,), _>(signals::RECOVERY_EVENT, ("event",));

            let _recovery_result =
                b.signal::<(u8, String), _>(signals::RECOVERY_RESULT, ("result", "why"));

            let _release_event = b.signal::<(u8,), _>(signals::RELEASE_EVENT, ("event",));

            let _release_result =
                b.signal::<(u8, String), _>(signals::RELEASE_RESULT, ("result", "why"));

            let _repo_compat_error = b.signal::<(Vec<String>, Vec<(String, String)>), _>(
                signals::REPO_COMPAT_ERROR,
                ("success", "failed"),
            );

            let _upgrade_event =
                b.signal::<(HashMap<String, String>,), _>(signals::PACKAGE_UPGRADE, ("event",));

            b.method(
                methods::CANCEL,
                (),
                (),
                |_ctx: &mut Context, daemon: &mut Daemon, _inputs: ()| {
                    daemon.cancel();
                    Ok(())
                },
            );

            b.method(
                methods::DISMISS_NOTIFICATION,
                ("dismiss",),
                ("dismissed",),
                |_ctx: &mut Context, daemon: &mut Daemon, (dismiss,): (u8,)| {
                    let event = DismissEvent::from_u8(dismiss)
                        .ok_or("dismiss value is out of range")
                        .map_err(|why| MethodErr::failed(&why))?;

                    daemon
                        .dismiss_notification(event)
                        .map(|v| (v,))
                        .map_err(|why| MethodErr::failed(&why))
                },
            );

            b.method(
                methods::FETCH_UPDATES,
                ("additional_packages", "download_only"),
                ("updates_available", "completed", "total"),
                |_ctx: &mut Context,
                 daemon: &mut Daemon,
                 (additional_packages, download_only): (Vec<String>, bool)| {
                    daemon
                        .set_status(
                            DaemonStatus::FetchingPackages,
                            move |daemon, already_active| {
                                if already_active {
                                    let (completed, total) =
                                        daemon.shared_state.fetching_state.load(Ordering::SeqCst);
                                    let completed = completed as u32;
                                    let total = total as u32;
                                    Ok((true, completed, total))
                                } else {
                                    async_io::block_on(
                                        daemon.fetch_updates(&additional_packages, download_only),
                                    )
                                    .map(|(x, t)| (x, 0u32, t))
                                    .map_err(|ref why| format_error(why.as_ref()))
                                }
                            },
                        )
                        .map_err(|why| MethodErr::failed(&why))
                },
            );

            b.method(
                methods::FETCH_UPDATES_STATUS,
                (),
                ("status", "why"),
                |_ctx: &mut Context, daemon: &mut Daemon, _inputs: ()| {
                    Ok(result_signal(daemon.last_known.fetch.as_ref()))
                },
            );

            b.method(
                methods::PACKAGE_UPGRADE,
                (),
                (),
                |_ctx: &mut Context, daemon: &mut Daemon, _inputs: ()| {
                    daemon.set_status(DaemonStatus::PackageUpgrade, move |daemon, active| {
                        if !active {
                            daemon
                                .package_upgrade()
                                .map_err(|ref why| format_error(why.as_ref()))
                                .map_err(|why| MethodErr::failed(&why))?;
                        }

                        Ok(())
                    })
                },
            );

            b.method(
                methods::RECOVERY_UPGRADE_FILE,
                ("path",),
                (),
                |_ctx: &mut Context, daemon: &mut Daemon, (path,): (String,)| {
                    daemon.set_status(DaemonStatus::RecoveryUpgrade, move |daemon, active| {
                        if !active {
                            daemon
                                .recovery_upgrade_file(&path)
                                .map_err(|ref why| format_error(why.as_ref()))
                                .map_err(|why| MethodErr::failed(&why))?;
                        }

                        Ok(())
                    })
                },
            );

            b.method(
                methods::RECOVERY_UPGRADE_RELEASE,
                ("version", "arch", "flags"),
                (),
                |_ctx: &mut Context,
                 daemon: &mut Daemon,
                 (version, arch, flags): (String, String, u8)| {
                    daemon.set_status(DaemonStatus::RecoveryUpgrade, move |daemon, active| {
                        if !active {
                            daemon
                                .recovery_upgrade_release(&version, &arch, flags)
                                .map_err(|ref why| format_error(why.as_ref()))
                                .map_err(|why| MethodErr::failed(&why))?;
                        }

                        Ok(())
                    })
                },
            );

            b.method(
                methods::RECOVERY_UPGRADE_RELEASE_STATUS,
                (),
                ("status", "why"),
                |_ctx: &mut Context, daemon: &mut Daemon, _inputs: ()| {
                    Ok(result_signal(daemon.last_known.recovery_upgrade.as_ref()))
                },
            );

            b.method(
                methods::RECOVERY_VERSION,
                (),
                ("version", "build"),
                |_ctx: &mut Context, daemon: &mut Daemon, _inputs: ()| {
                    daemon
                        .recovery_version()
                        .map(|v| (v.version, v.build))
                        .map_err(|why| MethodErr::failed(&why))
                },
            );

            b.method(
                methods::REFRESH_OS,
                ("input",),
                ("enabled",),
                |_ctx: &mut Context, daemon: &mut Daemon, (input,): (u8,)| {
                    let value = daemon
                        .refresh_os(match input {
                            1u8 => RefreshOp::Enable,
                            2u8 => RefreshOp::Disable,
                            _ => RefreshOp::Status,
                        })
                        .map_err(|why| MethodErr::failed(&why))?;

                    info!("responding with value of {}", value);

                    Ok((value,))
                },
            );

            b.method(
                methods::RELEASE_CHECK,
                ("development",),
                ("current", "next", "build", "urgent", "is_lts"),
                |_ctx: &mut Context, daemon: &mut Daemon, (development,): (bool,)| {
                    daemon
                        .release_check(development)
                        .map(|status| {
                            let is_lts = status.is_lts();
                            let mut urgent = -1;

                            if let Ok(release) =
                                crate::release_api::Release::get_release(status.current, "nvidia")
                            {
                                urgent = release.build as i16;
                            }

                            if status.current == "20.10" {
                                urgent = urgent.max(14);
                            }

                            (
                                String::from(status.current),
                                String::from(status.next),
                                status.build.status_code(),
                                urgent,
                                is_lts,
                            )
                        })
                        .map_err(|why| MethodErr::failed(&why))
                },
            );

            b.method(
                methods::RELEASE_UPGRADE,
                ("how", "from", "to"),
                (),
                |_ctx: &mut Context, daemon: &mut Daemon, (how, from, to): (u8, String, String)| {
                    daemon.set_status(DaemonStatus::ReleaseUpgrade, move |daemon, active| {
                        if !active {
                            daemon
                                .release_upgrade(how, &from, &to)
                                .map_err(|ref why| format_error(why.as_ref()))
                                .map_err(|why| MethodErr::failed(&why))?;
                        }

                        Ok(())
                    })
                },
            );

            b.method(
                methods::RELEASE_UPGRADE_FINALIZE,
                (),
                (),
                |_ctx: &mut Context, daemon: &mut Daemon, _inputs: ()| {
                    daemon.release_upgrade_finalize().map_err(|why| MethodErr::failed(&why))
                },
            );

            b.method(
                methods::RELEASE_UPGRADE_STATUS,
                (),
                ("status", "why"),
                |_ctx: &mut Context, daemon: &mut Daemon, _inputs: ()| {
                    Ok(result_signal(daemon.last_known.release_upgrade.as_ref()))
                },
            );

            b.method(
                methods::RELEASE_REPAIR,
                (),
                (),
                |_ctx: &mut Context, daemon: &mut Daemon, _inputs: ()| {
                    async_io::block_on(daemon.release_repair())
                        .map_err(|ref why| format_error(why.as_ref()))
                        .map_err(|why| MethodErr::failed(&why))
                },
            );

            b.method(
                methods::RESET,
                (),
                (),
                |_ctx: &mut Context, daemon: &mut Daemon, _inputs: ()| {
                    async_io::block_on(daemon.reset()).map_err(|why| MethodErr::failed(&why))
                },
            );

            b.method(
                methods::STATUS,
                (),
                ("status", "sub_status"),
                |_ctx: &mut Context, daemon: &mut Daemon, _inputs: ()| {
                    let status = daemon.shared_state.status.load(Ordering::SeqCst) as u8;
                    let sub_status = daemon.shared_state.sub_status.load(Ordering::SeqCst) as u8;

                    Ok((status, sub_status))
                },
            );

            b.method(
                methods::UPDATE_CHECK,
                (),
                ("status",),
                |_ctx: &mut Context, daemon: &mut Daemon, _inputs: ()| {
                    Ok((async_io::block_on(daemon.update_and_restart()),))
                },
            );
        });

        let (fg_receiver, receiver) = { (daemon.fg_rx.clone(), daemon.dbus_rx.clone()) };

        cr.insert(DBUS_PATH, &[iface_token], daemon);

        let cr = Arc::new(std::sync::Mutex::new(cr));

        let cr_ = cr.clone();
        connection.start_receive(
            MatchRule::new_method_call(),
            Box::new(move |msg, c| {
                eprintln!("handling message {:#?}", msg);
                cr_.lock().unwrap().handle_message(msg, c).unwrap();
                true
            }),
        );

        info!("daemon registered -- listening for new events");

        async_io::block_on(async move {
            release::cleanup().await;

            let path = dbus::strings::Path::from_slice("/com/system76/PopUpgrade\0").unwrap();

            loop {
                let _ = connection.process(std::time::Duration::from_millis(1000));
                let mut lock = cr.lock().unwrap();
                let daemon: &mut Daemon = lock.data_mut(&path).unwrap();

                if daemon.perform_upgrade {
                    let mut packages = vec!["pop-upgrade", "libpop-upgrade-gtk"];

                    if let Ok((_, mut policies)) =
                        AptCache::new().policy(&["libpop-upgrade-gtk-dev"]).await
                    {
                        if let Some(policy) = policies.next().await {
                            if policy.installed != "(none)" {
                                packages.push("libpop-upgrade-gtk-dev");
                            }
                        }
                    }

                    self_upgrade(&packages).await;
                }

                if let Some(status) = sighandler::status() {
                    info!("received a '{}' signal", status);

                    use sighandler::Signal::{TermStop, Terminate};

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
                                daemon.release_upgrade = Some(state);
                            }

                            daemon.last_known.release_upgrade = result;
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
                                let message = Self::signal_message(signals::PACKAGE_FETCH_RESULT)
                                    .append2(status, why);

                                daemon.last_known.fetch = result;
                                message
                            }
                            SignalEvent::Fetched(name, completed, total) => Self::signal_message(
                                signals::PACKAGE_FETCHED,
                            )
                            .append3(name.as_str(), completed, total),
                            SignalEvent::Fetching(name) => {
                                Self::signal_message(signals::PACKAGE_FETCHING)
                                    .append1(name.as_str())
                            }
                            SignalEvent::NoConnection => {
                                Self::signal_message(signals::NO_CONNECTION)
                            }
                            SignalEvent::RecoveryDownloadProgress(progress, total) => {
                                Self::signal_message(signals::RECOVERY_DOWNLOAD_PROGRESS)
                                    .append2(progress, total)
                            }
                            SignalEvent::RecoveryUpgradeEvent(event) => {
                                Self::signal_message(signals::RECOVERY_EVENT).append1(event as u8)
                            }
                            SignalEvent::RecoveryUpgradeResult(result) => {
                                let (status, why) = result_signal(result.as_ref());
                                let message = Self::signal_message(signals::RECOVERY_RESULT)
                                    .append2(status, why);

                                daemon.last_known.recovery_upgrade = result;
                                message
                            }
                            SignalEvent::ReleaseUpgradeEvent(event) => {
                                Self::signal_message(signals::RELEASE_EVENT).append1(event as u8)
                            }
                            SignalEvent::Upgrade(ref event) => {
                                Self::signal_message(signals::PACKAGE_UPGRADE)
                                    .append1(event.clone().into_dbus_map())
                            }
                        }
                    });
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
                dismiss_file_create(status.next)?;

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
        borrows.extend(additional_packages.iter().map(String::as_str));

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

        self.shared_state.cancel.store(true, Ordering::SeqCst);
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
            arch:    if arch.is_empty() { None } else { Some(arch.into()) },
            flags:   RecoveryReleaseFlags::from_bits_truncate(flags),
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
                return Err(format_error(why));
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
        if recovery::recovery_exists()? {
            self.recovery_upgrade_release(to, "", 0)?;
        }

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

        self.shared_state.status.store(DaemonStatus::Inactive, Ordering::SeqCst);
        self.shared_state.sub_status.store(0, Ordering::SeqCst);
        self.shared_state.fetching_state.store((0, 0), Ordering::SeqCst);
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
        let already_active = self.shared_state.status.swap(status, Ordering::SeqCst) == status;
        match func(self, already_active) {
            Ok(value) => Ok(value),
            Err(why) => {
                self.shared_state.status.store(DaemonStatus::Inactive, Ordering::SeqCst);
                Err(why)
            }
        }
    }

    fn signal_message(name: &'static str) -> Message {
        Message::new_signal(DBUS_PATH, DBUS_NAME, name).unwrap()
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
