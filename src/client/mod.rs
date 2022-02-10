use crate::{
    daemon::*,
    recovery::{RecoveryEvent, ReleaseFlags as RecoveryReleaseFlags},
    release::{RefreshOp, UpgradeEvent, UpgradeMethod},
    sighandler, DBUS_IFACE, DBUS_NAME, DBUS_PATH,
};

use dbus::{
    arg::messageitem::{MessageItem, MessageItemArray},
    ffidisp::{Connection, ConnectionItem},
    Message, Signature,
};

use num_traits::FromPrimitive;
use std::collections::HashMap;
use thiserror::Error;

const TIMEOUT: i32 = 0x7fff_ffff;

// Information about the current fetch progress.
#[derive(Clone, Debug)]
pub struct FetchStatus {
    pub package:   Box<str>,
    pub completed: u32,
    pub total:     u32,
}

/// Data for tracking progress of an action.
#[derive(Clone, Debug)]
pub struct Progress {
    pub progress: u64,
    pub total:    u64,
}

/// Contains information about good and bad repositories.
#[derive(Clone, Debug)]
pub struct RepoCompatError {
    pub success: Vec<String>,
    pub failure: Vec<(String, String)>,
}

/// A signal received by the daemon.
pub enum Signal {
    NoConnection,
    PackageFetchResult(Status),
    PackageFetched(FetchStatus),
    PackageFetching(Box<str>),
    PackageUpgrade(HashMap<Box<str>, Box<str>>),
    RecoveryDownloadProgress(Progress),
    RecoveryEvent(RecoveryEvent),
    RecoveryResult(Status),
    ReleaseResult(Status),
    ReleaseEvent(UpgradeEvent),
}

/// Designates if the signal event loop should continue listening for signals.
#[derive(Clone, Debug)]
pub struct Continue(pub bool);

/// The status of the daemon that was retrieved.
#[derive(Clone, Debug)]
pub struct DaemonStatus {
    pub status:     u8,
    pub sub_status: u8,
}

/// Information about available system updates.
#[derive(Clone, Debug)]
pub struct Fetched {
    pub updates_available: bool,
    pub completed:         u32,
    pub total:             u32,
}

/// The version of the recovery partition's image.
#[derive(Clone, Debug)]
pub struct RecoveryVersion {
    pub version: Box<str>,
    pub build:   i16,
}

/// Information about the current and next release.
///
/// The build is set to `-1` if the next release is
/// not available.
#[derive(Clone, Debug)]
pub struct ReleaseInfo {
    pub current: Box<str>,
    pub next:    Box<str>,
    pub build:   i16,
    pub urgent:  Option<u16>,
    pub is_lts:  bool,
}

/// The status of an action, and a description of why.
#[derive(Clone, Debug)]
pub struct Status {
    pub status: u8,
    pub why:    Box<str>,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to add match on client connection")]
    AddMatch(#[source] dbus::Error),

    #[error("argument mismatch in {} method", _0)]
    ArgumentMismatch(&'static str, #[source] dbus::arg::TypeMismatchError),

    #[error("calling {} method failed", _0)]
    Call(&'static str, #[source] dbus::Error),

    #[error("unable to establish dbus connection")]
    Connection(#[source] dbus::Error),

    #[error("daemon status integer was outside the acceptable range of values")]
    DaemonStatusOutOfRange,

    #[error("failed to create {} method call", _0)]
    NewMethodCall(&'static str, String),
}

pub struct Client {
    pub bus: Connection,
}

impl Client {
    /// Attempts to create a new dbus connection to the upgrade daemon.
    pub fn new() -> Result<Self, Error> {
        fn add_match(cbus: &Connection, member: &'static str) -> Result<(), Error> {
            cbus.add_match(&format!("interface='{}',member='{}'", DBUS_IFACE, member))
                .map_err(Error::AddMatch)?;

            Ok(())
        }

        Connection::new_system().map_err(Error::Connection).and_then(|bus| {
            {
                let bus = &bus;
                add_match(bus, signals::NO_CONNECTION)?;
                add_match(bus, signals::PACKAGE_FETCH_RESULT)?;
                add_match(bus, signals::PACKAGE_FETCHED)?;
                add_match(bus, signals::PACKAGE_FETCHING)?;
                add_match(bus, signals::PACKAGE_UPGRADE)?;
                add_match(bus, signals::RECOVERY_DOWNLOAD_PROGRESS)?;
                add_match(bus, signals::RECOVERY_RESULT)?;
                add_match(bus, signals::RECOVERY_EVENT)?;
                add_match(bus, signals::RELEASE_RESULT)?;
                add_match(bus, signals::RELEASE_EVENT)?;
                add_match(bus, signals::REPO_COMPAT_ERROR)?;
            }

            Ok(Client { bus })
        })
    }

    /// Cancel the active process which is in progress
    pub fn cancel(&self) -> Result<(), Error> {
        self.call_method(methods::CANCEL, |m| m)?;
        Ok(())
    }

    /// Dismiss future desktop notifications for the currently-available upgrade.
    pub fn dismiss_notification(&self, event: DismissEvent) -> Result<bool, Error> {
        self.call_method(methods::DISMISS_NOTIFICATION, |m| m.append1(event as u8))?
            .read1::<bool>()
            .map_err(|why| Error::ArgumentMismatch(methods::DISMISS_NOTIFICATION, why))
    }

    /// Initiates fetching system updates (not release updates).
    ///
    /// By default, the system is updated once updates have been fetched. This
    /// can be disabled by setting the `download_only` argument to `false`.
    pub fn fetch_updates(
        &self,
        additional_packages: Vec<String>,
        download_only: bool,
    ) -> Result<Fetched, Error> {
        let packages = MessageItemArray::new(
            additional_packages.into_iter().map(MessageItem::from).collect(),
            Signature::from_slice("as\0").unwrap(),
        )
        .unwrap();

        let packages = MessageItem::Array(packages);

        let cb = move |message: Message| message.append2(&packages, download_only);

        self.call_method(methods::FETCH_UPDATES, cb)?
            .read3::<bool, u32, u32>()
            .map_err(|why| Error::ArgumentMismatch(methods::FETCH_UPDATES, why))
            .map(|(updates_available, completed, total)| Fetched {
                updates_available,
                completed,
                total,
            })
    }

    /// Retrieves the last known status of a system update.
    pub fn fetch_updates_status(&self) -> Result<Status, Error> {
        self.call_method(methods::FETCH_UPDATES_STATUS, |m| m)?
            .read2::<u8, &str>()
            .map_err(|why| Error::ArgumentMismatch(methods::FETCH_UPDATES_STATUS, why))
            .map(|(status, why)| Status { status, why: why.into() })
    }

    /// Initiates upgrading the system packages.
    pub fn package_upgrade(&self) -> Result<(), Error> {
        self.call_method(methods::PACKAGE_UPGRADE, |m| m)?;
        Ok(())
    }

    /// Initiates upgrading the recovery partition via a recovery image file.
    pub fn recovery_upgrade_file<P: AsRef<str>>(&self, path: P) -> Result<u8, Error> {
        self.call_method(methods::RECOVERY_UPGRADE_FILE, move |m| m.append1(path.as_ref()))?
            .read1::<u8>()
            .map_err(|why| Error::ArgumentMismatch(methods::RECOVERY_UPGRADE_FILE, why))
    }

    /// Initiates upgrading the recovery partition via the release API
    pub fn recovery_upgrade_release(
        &self,
        version: &str,
        arch: &str,
        flags: RecoveryReleaseFlags,
    ) -> Result<(), Error> {
        let cb = move |message: Message| message.append3(version, arch, flags.bits());

        self.call_method(methods::RECOVERY_UPGRADE_RELEASE, cb)?;
        Ok(())
    }

    /// Retrieves the last known status of a recovery upgrade.
    pub fn recovery_upgrade_release_status(&self) -> Result<Status, Error> {
        self.call_method(methods::RECOVERY_UPGRADE_RELEASE_STATUS, |m| m)?
            .read2::<u8, &str>()
            .map_err(|why| Error::ArgumentMismatch(methods::RECOVERY_UPGRADE_RELEASE_STATUS, why))
            .map(|(status, why)| Status { status, why: why.into() })
    }

    /// Fetches the version of the recovery partition currently-installed.
    pub fn recovery_version(&self) -> Result<RecoveryVersion, Error> {
        self.call_method(methods::RECOVERY_VERSION, |m| m)?
            .read2::<&str, i16>()
            .map_err(|why| Error::ArgumentMismatch(methods::RECOVERY_VERSION, why))
            .map(|(version, build)| RecoveryVersion { version: version.into(), build })
    }

    /// Configures the system to perform a system refresh on the next system boot.
    pub fn refresh_os(&self, operation: RefreshOp) -> Result<bool, Error> {
        self.call_method(methods::REFRESH_OS, |m| m.append1(operation as u8))?
            .read1::<bool>()
            .map_err(|why| Error::ArgumentMismatch(methods::REFRESH_OS, why))
    }

    /// Check the current release information
    ///
    /// Used to determine if a release upgrade is available.
    pub fn release_check(&self, development: bool) -> Result<ReleaseInfo, Error> {
        self.call_method(methods::RELEASE_CHECK, |m| m.append1(development))?
            .read5::<&str, &str, i16, i16, bool>()
            .map_err(|why| Error::ArgumentMismatch(methods::RELEASE_CHECK, why))
            .map(|(current, next, build, urgent, is_lts)| ReleaseInfo {
                current: current.into(),
                next: next.into(),
                build,
                urgent: if urgent > -1 { Some(urgent as u16) } else { None },
                is_lts,
            })
    }

    /// Initiates a release upgrade using the given method.
    pub fn release_upgrade(&self, how: UpgradeMethod, from: &str, to: &str) -> Result<(), Error> {
        self.call_method(methods::RELEASE_UPGRADE, move |m| m.append3(how as u8, from, to))?;

        Ok(())
    }

    pub fn release_upgrade_finalize(&self) -> Result<(), Error> {
        self.call_method(methods::RELEASE_UPGRADE_FINALIZE, |m| m)?;
        Ok(())
    }

    /// Retrieves the last known status of a release upgrade.
    pub fn release_upgrade_status(&self) -> Result<Status, Error> {
        self.call_method(methods::RELEASE_UPGRADE_STATUS, |m| m)?
            .read2::<u8, &str>()
            .map_err(|why| Error::ArgumentMismatch(methods::RELEASE_UPGRADE_STATUS, why))
            .map(|(status, why)| Status { status, why: why.into() })
    }

    /// Attempts to repair any system issues detected.
    pub fn release_repair(&self) -> Result<(), Error> {
        self.call_method(methods::RELEASE_REPAIR, |m| m)?;
        Ok(())
    }

    /// Reset the daemon to its initial state, and clean up any changes.
    pub fn reset(&self) -> Result<(), Error> {
        self.call_method(methods::RESET, |m| m)?;
        Ok(())
    }

    /// Retrieves the status of the daemon.
    pub fn status(&self) -> Result<DaemonStatus, Error> {
        self.call_method(methods::STATUS, |m| m)?
            .read2::<u8, u8>()
            .map_err(|why| Error::ArgumentMismatch(methods::STATUS, why))
            .map(|(status, sub_status)| DaemonStatus { status, sub_status })
    }

    pub fn update_and_restart(&self) -> Result<bool, Error> {
        self.call_method(methods::UPDATE_CHECK, |m| m)?
            .read1::<u8>()
            .map_err(|why| Error::ArgumentMismatch(methods::UPDATE_CHECK, why))
            .map(|v| v == 1)
    }

    /// Verifies if a recovery partition exists.
    pub fn recovery_exists(&self) -> bool { crate::recovery::recovery_exists().unwrap_or(false) }

    /// An event loop for listening to signals from the daemon.
    pub fn event_listen(
        &self,
        status_func: fn(&Client) -> Result<Status, Error>,
        mut log_cb: impl FnMut(Status),
        mut event: impl FnMut(&Self, Signal) -> Result<Continue, Error>,
    ) -> Result<(), Error> {
        let mut break_on_next = false;
        for item in self.bus.iter(500) {
            if sighandler::status().is_some() {
                let _ = self.cancel();
            }

            if let ConnectionItem::Nothing = item {
                if self.is_inactive()? {
                    if break_on_next {
                        log_cb(status_func(self)?);

                        break;
                    }

                    break_on_next = true;
                }
            } else if let Some(signal) = filter_signal(item) {
                let signal = match &*signal.member().unwrap() {
                    signals::NO_CONNECTION => Signal::NoConnection,
                    signals::PACKAGE_FETCH_RESULT => signal
                        .read2::<u8, String>()
                        .map(|(status, why)| Status { status, why: why.into() })
                        .map(Signal::PackageFetchResult)
                        .map_err(|why| {
                            Error::ArgumentMismatch(signals::PACKAGE_FETCH_RESULT, why)
                        })?,
                    signals::PACKAGE_FETCHED => signal
                        .read3::<String, u32, u32>()
                        .map(|(package, completed, total)| FetchStatus {
                            package: package.into(),
                            completed,
                            total,
                        })
                        .map(Signal::PackageFetched)
                        .map_err(|why| Error::ArgumentMismatch(signals::PACKAGE_FETCHED, why))?,
                    signals::PACKAGE_FETCHING => signal
                        .read1::<String>()
                        .map(|package| Signal::PackageFetching(Box::from(package)))
                        .map_err(|why| Error::ArgumentMismatch(signals::PACKAGE_FETCHING, why))?,
                    signals::PACKAGE_UPGRADE => signal
                        .read1::<HashMap<String, String>>()
                        .map_err(|why| Error::ArgumentMismatch(signals::PACKAGE_UPGRADE, why))
                        .map(|upgrade| {
                            upgrade
                                .into_iter()
                                .map(|(key, value)| (Box::from(key), Box::from(value)))
                                .collect::<HashMap<Box<str>, Box<str>>>()
                        })
                        .map(Signal::PackageUpgrade)?,
                    signals::RECOVERY_DOWNLOAD_PROGRESS => signal
                        .read2::<u64, u64>()
                        .map_err(|why| {
                            Error::ArgumentMismatch(signals::RECOVERY_DOWNLOAD_PROGRESS, why)
                        })
                        .map(|(progress, total)| Progress { progress, total })
                        .map(Signal::RecoveryDownloadProgress)?,
                    signals::RECOVERY_EVENT => signal
                        .read1::<u8>()
                        .map_err(|why| Error::ArgumentMismatch(signals::RECOVERY_EVENT, why))
                        .map(|event| {
                            RecoveryEvent::from_u8(event).expect("unexpected recovery event value")
                        })
                        .map(Signal::RecoveryEvent)?,
                    signals::RECOVERY_RESULT => signal
                        .read2::<u8, String>()
                        .map_err(|why| Error::ArgumentMismatch(signals::RECOVERY_RESULT, why))
                        .map(|(status, why)| Status { status, why: why.into() })
                        .map(Signal::RecoveryResult)?,
                    signals::RELEASE_EVENT => signal
                        .read1::<u8>()
                        .map_err(|why| Error::ArgumentMismatch(signals::RELEASE_EVENT, why))
                        .map(|event| {
                            UpgradeEvent::from_u8(event).expect("unexpected upgrade event value")
                        })
                        .map(Signal::ReleaseEvent)?,
                    signals::RELEASE_RESULT => signal
                        .read2::<u8, String>()
                        .map_err(|why| Error::ArgumentMismatch(signals::RELEASE_RESULT, why))
                        .map(|(status, why)| Status { status, why: why.into() })
                        .map(Signal::ReleaseResult)?,
                    _ => continue,
                };

                if !event(self, signal)?.0 {
                    break;
                }
            }
        }

        Ok(())
    }

    fn call_method<F: FnMut(Message) -> Message>(
        &self,
        method: &'static str,
        mut append_args: F,
    ) -> Result<Message, Error> {
        let mut m = Message::new_method_call(DBUS_NAME, DBUS_PATH, DBUS_IFACE, method)
            .map_err(|why| Error::NewMethodCall(method, why))?;

        m = append_args(m);

        self.bus.send_with_reply_and_block(m, TIMEOUT).map_err(|why| Error::Call(method, why))
    }

    fn is_inactive(&self) -> Result<bool, Error> { self.status().map(|status| status.status == 0) }
}

fn filter_signal(ci: ConnectionItem) -> Option<Message> {
    if let ConnectionItem::Signal(ci) = ci {
        Some(ci)
    } else {
        None
    }
}
