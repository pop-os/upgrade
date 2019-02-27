use crate::apt_wrappers::AptUpgradeEvent;
use crate::recovery::{RecoveryError, RecoveryEvent};
use crate::release::{ReleaseError, UpgradeEvent};
use std::fmt::{self, Display, Formatter};

// Signals supported by the daemon.
pub const PACKAGE_FETCH_RESULT: &str = "PackageFetchResult";
pub const PACKAGE_FETCHING: &str = "PackageFetching";
pub const PACKAGE_FETCHED: &str = "PackageFetched";

pub const PACKAGE_UPGRADE: &str = "PackageUpgrade";

pub const RECOVERY_DOWNLOAD_PROGRESS: &str = "RecoveryDownloadProgress";
pub const RECOVERY_EVENT: &str = "RecoveryUpgradeEvent";
pub const RECOVERY_RESULT: &str = "RecoveryUpgradeResult";

pub const RELEASE_EVENT: &str = "ReleaseUpgradeEvent";
pub const RELEASE_RESULT: &str = "ReleaseUpgradeResult";

#[derive(Debug)]
pub enum SignalEvent {
    FetchResult(Result<(), ReleaseError>),
    Fetched(String, u32, u32),
    Fetching(String),
    RecoveryDownloadProgress(u64, u64),
    RecoveryUpgradeEvent(RecoveryEvent),
    RecoveryUpgradeResult(Result<(), RecoveryError>),
    ReleaseUpgradeEvent(UpgradeEvent),
    ReleaseUpgradeResult(Result<(), ReleaseError>),
    Upgrade(AptUpgradeEvent),
}

impl Display for SignalEvent {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        use self::SignalEvent::*;
        match self {
            FetchResult(result) => write!(fmt, "fetch result: {:?}", result),
            Fetched(package, progress, total) => {
                write!(fmt, "fetched {}/{}: {}", progress, total, package)
            }
            Fetching(package) => write!(fmt, "fetching {}", package),
            RecoveryDownloadProgress(progress, total) => {
                write!(fmt, "recovery download: {}/{} MiB", progress / 1024, total / 1024)
            }
            RecoveryUpgradeEvent(event) => {
                write!(fmt, "recovery upgrade: {}", <&'static str>::from(*event))
            }
            RecoveryUpgradeResult(result) => write!(fmt, "recovery upgrade result: {:?}", result),
            ReleaseUpgradeEvent(event) => {
                write!(fmt, "release upgrade: {}", <&'static str>::from(*event))
            }
            ReleaseUpgradeResult(result) => write!(fmt, "release upgrade result: {:?}", result),
            Upgrade(event) => write!(fmt, "package upgrade: {}", event),
        }
    }
}
