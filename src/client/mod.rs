use crate::{
    daemon::DaemonStatus as PrimaryStatus,
    daemon::*,
    misc,
    recovery::{RecoveryEvent, ReleaseFlags as RecoveryReleaseFlags},
    release::{RefreshOp, UpgradeEvent, UpgradeMethod},
    DBUS_IFACE, DBUS_NAME, DBUS_PATH,
};

use num_traits::FromPrimitive;

use dbus::{
    self, BusType, Connection, ConnectionItem, Message, MessageItem, MessageItemArray, Signature,
};

use std::{collections::HashMap, iter, path::Path};

const TIMEOUT: i32 = 3000;

const FETCH_RESULT_STR: &str = "Package fetch status";
const FETCH_RESULT_SUCCESS: &str = "cargo has been loaded successfully";
const FETCH_RESULT_ERROR: &str = "package-fetching aborted";

const RECOVERY_RESULT_STR: &str = "Recovery upgrade status";
const RECOVERY_RESULT_SUCCESS: &str = "recovery partition refueled and ready to go";
const RECOVERY_RESULT_ERROR: &str = "recovery upgrade aborted";

const UPGRADE_RESULT_STR: &str = "Release upgrade status";
const UPGRADE_RESULT_SUCCESS: &str = "systems are go for launch: reboot now";
const UPGRADE_RESULT_ERROR: &str = "release upgrade aborted";

#[derive(Clone, Debug)]
pub struct FetchStatus {
    pub package: Box<str>,
    pub completed: u32,
    pub total: u32,
}

#[derive(Clone, Debug)]
pub struct Progress {
    pub progress: u64,
    pub total: u64,
}

#[derive(Clone, Debug)]
pub struct RepoCompatError {
    pub success: Vec<String>,
    pub failure: Vec<(String, String)>,
}

pub enum Signal {
    PackageFetchResult(Status),
    PackageFetched(FetchStatus),
    PackageFetching(Box<str>),
    PackageUpgrade(HashMap<Box<str>, Box<str>>),
    RecoveryDownloadProgress(Progress),
    RecoveryEvent(RecoveryEvent),
    RecoveryResult(Status),
    ReleaseResult(Status),
    ReleaseEvent(UpgradeEvent),
    RepoCompatError(RepoCompatError),
}

#[derive(Clone, Debug)]
pub struct Continue(pub bool);

#[derive(Clone, Debug)]
pub struct DaemonStatus {
    pub status: u8,
    pub sub_status: u8,
}

#[derive(Clone, Debug)]
pub struct Fetched {
    pub updates_available: bool,
    pub completed: u32,
    pub total: u32,
}

#[derive(Clone, Debug)]
pub struct RecoveryVersion {
    pub version: Box<str>,
    pub build: u16,
}

#[derive(Clone, Debug)]
pub struct ReleaseInfo {
    pub current: Box<str>,
    pub next: Box<str>,
    pub build: i16,
}

#[derive(Clone, Debug)]
pub struct Status {
    pub status: u8,
    pub why: Box<str>,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(display = "failed to add match on client connection")]
    AddMatch(#[error(cause)] dbus::Error),
    #[error(display = "argument mismatch in {} method", _0)]
    ArgumentMismatch(&'static str, #[error(cause)] dbus::arg::TypeMismatchError),
    #[error(display = "calling {} method failed", _0)]
    Call(&'static str, #[error(cause)] dbus::Error),
    #[error(display = "unable to establish dbus connection")]
    Connection(#[error(cause)] dbus::Error),
    #[error(display = "daemon status integer was outside the acceptable range of values")]
    DaemonStatusOutOfRange,
    #[error(display = "failed to create {} method call", _0)]
    NewMethodCall(&'static str, String),
}

pub struct Client {
    pub bus: Connection,
}

impl Client {
    pub fn new() -> Result<Self, Error> {
        fn add_match(cbus: &Connection, member: &'static str) -> Result<(), Error> {
            cbus.add_match(&format!("interface='{}',member='{}'", DBUS_IFACE, member))
                .map_err(Error::AddMatch)?;

            Ok(())
        }

        Connection::get_private(BusType::System)
            .map_err(Error::Connection)
            .and_then(|bus| {
                {
                    let bus = &bus;
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

    pub fn fetch_updates(
        &self,
        additional_packages: Vec<String>,
        download_only: bool,
    ) -> Result<Fetched, Error> {
        let packages = MessageItemArray::new(
            additional_packages
                .into_iter()
                .map(MessageItem::from)
                .collect(),
            Signature::from_slice(b"as\0").unwrap(),
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

    pub fn fetch_updates_status(&self) -> Result<Status, Error> {
        self.call_method(methods::FETCH_UPDATES_STATUS, |m| m)?
            .read2::<u8, &str>()
            .map_err(|why| Error::ArgumentMismatch(methods::FETCH_UPDATES_STATUS, why))
            .map(|(status, why)| Status {
                status,
                why: why.into(),
            })
    }

    pub fn package_upgrade(&self) -> Result<(), Error> {
        self.call_method(methods::PACKAGE_UPGRADE, |m| m)?;
        Ok(())
    }

    pub fn recovery_upgrade_file<P: AsRef<str>>(&self, path: P) -> Result<u8, Error> {
        self.call_method(methods::RECOVERY_UPGRADE_FILE, move |m| {
            m.append1(path.as_ref())
        })?
        .read1::<u8>()
        .map_err(|why| Error::ArgumentMismatch(methods::RECOVERY_UPGRADE_FILE, why))
    }

    pub fn recovery_upgrade_release(
        &self,
        version: &str,
        arch: &str,
        flags: RecoveryReleaseFlags,
    ) -> Result<u8, Error> {
        let cb = move |message: Message| message.append3(version, arch, flags.bits());

        self.call_method(methods::RECOVERY_UPGRADE_RELEASE, cb)?
            .read1::<u8>()
            .map_err(|why| Error::ArgumentMismatch(methods::RECOVERY_UPGRADE_RELEASE, why))
    }

    pub fn recovery_upgrade_release_status(&self) -> Result<Status, Error> {
        self.call_method(methods::RECOVERY_UPGRADE_RELEASE_STATUS, |m| m)?
            .read2::<u8, &str>()
            .map_err(|why| Error::ArgumentMismatch(methods::RECOVERY_UPGRADE_RELEASE_STATUS, why))
            .map(|(status, why)| Status {
                status,
                why: why.into(),
            })
    }

    pub fn recovery_version(&self) -> Result<RecoveryVersion, Error> {
        self.call_method(methods::RECOVERY_VERSION, |m| m)?
            .read2::<&str, u16>()
            .map_err(|why| Error::ArgumentMismatch(methods::RECOVERY_VERSION, why))
            .map(|(version, build)| RecoveryVersion {
                version: version.into(),
                build,
            })
    }

    pub fn refresh_os(&self, operation: RefreshOp) -> Result<bool, Error> {
        self.call_method(methods::REFRESH_OS, |m| m.append1(operation as u8))?
            .read1::<bool>()
            .map_err(|why| Error::ArgumentMismatch(methods::REFRESH_OS, why))
    }

    pub fn release_check(&self) -> Result<ReleaseInfo, Error> {
        self.call_method(methods::RELEASE_CHECK, |m| m)?
            .read3::<&str, &str, i16>()
            .map_err(|why| Error::ArgumentMismatch(methods::RELEASE_CHECK, why))
            .map(|(current, next, build)| ReleaseInfo {
                current: current.into(),
                next: next.into(),
                build,
            })
    }

    pub fn release_upgrade(&self, how: UpgradeMethod, from: &str, to: &str) -> Result<(), Error> {
        self.call_method(methods::RELEASE_UPGRADE, move |m| {
            m.append3(how as u8, from, to)
        })?;

        Ok(())
    }

    pub fn release_upgrade_status(&self) -> Result<Status, Error> {
        self.call_method(methods::RELEASE_UPGRADE_STATUS, |m| m)?
            .read2::<u8, &str>()
            .map_err(|why| Error::ArgumentMismatch(methods::RELEASE_UPGRADE_STATUS, why))
            .map(|(status, why)| Status {
                status,
                why: why.into(),
            })
    }

    pub fn release_repair(&self) -> Result<(), Error> {
        self.call_method(methods::RELEASE_REPAIR, |m| m)?;
        Ok(())
    }

    pub fn status(&self) -> Result<DaemonStatus, Error> {
        self.call_method(methods::STATUS, |m| m)?
            .read2::<u8, u8>()
            .map_err(|why| Error::ArgumentMismatch(methods::STATUS, why))
            .map(|(status, sub_status)| DaemonStatus { status, sub_status })
    }

    pub fn repo_modify<S: AsRef<str>>(
        &self,
        repos: impl Iterator<Item = (S, bool)>,
    ) -> Result<(), Error> {
        let repos = repos.map(|(url, keep)| {
            MessageItem::DictEntry(Box::new(url.as_ref().into()), Box::new(keep.into()))
        });

        let array = MessageItem::Array(
            MessageItemArray::new(
                repos.collect::<Vec<_>>(),
                Signature::from_slice(b"a{sb}\0").unwrap(),
            )
            .unwrap(),
        );

        self.call_method(methods::REPO_MODIFY, move |m| m.append1(&array))?;
        Ok(())
    }

    pub fn call_method<F: FnMut(Message) -> Message>(
        &self,
        method: &'static str,
        mut append_args: F,
    ) -> Result<Message, Error> {
        let mut m = Message::new_method_call(DBUS_NAME, DBUS_PATH, DBUS_IFACE, method)
            .map_err(|why| Error::NewMethodCall(method, why))?;

        m = append_args(m);

        self.bus
            .send_with_reply_and_block(m, TIMEOUT)
            .map_err(|why| Error::Call(method, why))
    }

    pub fn event_listen(
        &self,
        expected_status: PrimaryStatus,
        status_func: fn(&Client) -> Result<Status, Error>,
        log_cb: impl Fn(Status),
        mut event: impl FnMut(&Self, Signal) -> Result<Continue, Error>,
    ) -> Result<(), Error> {
        for item in self.bus.iter(3000) {
            if let ConnectionItem::Nothing = item {
                if !self.status_is(expected_status)? {
                    log_cb(status_func(self)?);

                    break;
                }
            } else if let Some(signal) = filter_signal(item) {
                let signal = match &*signal.member().unwrap() {
                    signals::PACKAGE_FETCH_RESULT => signal
                        .read2::<u8, String>()
                        .map(|(status, why)| Status {
                            status,
                            why: why.into(),
                        })
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
                        .map(|(status, why)| Status {
                            status,
                            why: why.into(),
                        })
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
                        .map(|(status, why)| Status {
                            status,
                            why: why.into(),
                        })
                        .map(Signal::ReleaseResult)?,
                    signals::REPO_COMPAT_ERROR => signal
                        .read2::<Vec<String>, Vec<(String, String)>>()
                        .map_err(|why| Error::ArgumentMismatch(signals::REPO_COMPAT_ERROR, why))
                        .map(|(success, failure)| RepoCompatError { success, failure })
                        .map(Signal::RepoCompatError)?,
                    _ => unimplemented!(),
                };

                if !event(self, signal)?.0 {
                    break;
                }
            }
        }

        Ok(())
    }

    fn status_is(&self, expected: PrimaryStatus) -> Result<bool, Error> {
        let status = self.status()?;
        let status = PrimaryStatus::from_u8(status.status).ok_or(Error::DaemonStatusOutOfRange)?;
        Ok(status == expected)
    }
}

fn filter_signal(ci: ConnectionItem) -> Option<Message> {
    if let ConnectionItem::Signal(ci) = ci {
        Some(ci)
    } else {
        None
    }
}
