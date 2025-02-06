use num_derive::FromPrimitive;
use std::{
    fmt::{self, Display},
    path::{Path, PathBuf},
};

pub const DEVELOPMENT_RELEASE_FILE: &str = "/etc/pop-upgrade/devel";
pub const RELEASE_FETCH_FILE: &str = "/pop_preparing_release_upgrade";
pub const RESTART_SCHEDULED: &str = "/var/lib/pop-upgrade/restarting";
pub const STARTUP_UPGRADE_FILE: &str = "/pop-upgrade";

pub fn development_releases_enabled() -> bool {
    Path::new(DEVELOPMENT_RELEASE_FILE).exists()
}

pub fn recovery_exists() -> std::io::Result<bool> {
    let mounts = proc_mounts::MountIter::new()?;

    for mount in mounts {
        let mount = mount?;
        if mount.dest == Path::new("/recovery") {
            return Ok(true);
        }
    }

    Ok(false)
}

pub fn reboot_is_ready() -> bool {
    Path::new(STARTUP_UPGRADE_FILE).exists()
}

pub fn upgrade_in_progress() -> bool {
    Path::new(STARTUP_UPGRADE_FILE).exists() || Path::new(RELEASE_FETCH_FILE).exists()
}

/// The status of the daemon that was retrieved.
#[derive(Clone, Debug)]
pub struct DaemonState {
    pub status: u8,
    pub sub_status: u8,
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, FromPrimitive, PartialEq)]
pub enum DaemonStatus {
    Inactive = 0,
    FetchingPackages = 1,
    RecoveryUpgrade = 2,
    ReleaseUpgrade = 3,
    PackageUpgrade = 4,
}

unsafe impl bytemuck::NoUninit for DaemonStatus {}

impl From<DaemonStatus> for &'static str {
    fn from(status: DaemonStatus) -> Self {
        match status {
            DaemonStatus::Inactive => "inactive",
            DaemonStatus::FetchingPackages => "fetching package updates",
            DaemonStatus::RecoveryUpgrade => "upgrading recovery partition",
            DaemonStatus::ReleaseUpgrade => "upgrading distribution release",
            DaemonStatus::PackageUpgrade => "upgrading packages",
        }
    }
}

impl Display for DaemonStatus {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.write_str(<&'static str>::from(*self))
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, FromPrimitive, PartialEq)]
pub enum DismissEvent {
    ByTimestamp = 1,
    ByUser = 2,
    Unset = 3,
}

/// Information about available system updates.
#[derive(Clone, Debug)]
pub struct Fetched {
    pub updates_available: bool,
    pub completed: u32,
    pub total: u32,
}

/// Information about the current fetch progress.
#[derive(Clone, Debug)]
pub struct FetchStatus {
    pub package: Box<str>,
    pub completed: u32,
    pub total: u32,
}

/// Data for tracking progress of an action.
#[derive(Clone, Debug)]
pub struct Progress {
    pub progress: u64,
    pub total: u64,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, FromPrimitive, PartialEq)]
pub enum RecoveryEvent {
    Fetching = 1,
    Verifying = 2,
    Syncing = 3,
    Complete = 4,
}

impl From<RecoveryEvent> for &'static str {
    fn from(event: RecoveryEvent) -> Self {
        match event {
            RecoveryEvent::Fetching => "fetching recovery files",
            RecoveryEvent::Syncing => "syncing recovery files with recovery partition",
            RecoveryEvent::Verifying => "verifying checksums of fetched files",
            RecoveryEvent::Complete => "recovery partition upgrade completed",
        }
    }
}

/// The version of the recovery partition's image.
#[derive(Clone, Debug)]
pub struct RecoveryVersion {
    pub version: Box<str>,
    pub build: i16,
}

#[repr(u8)]
#[derive(Copy, Clone, Debug)]
pub enum RefreshOp {
    Status = 0,
    Enable = 1,
    Disable = 2,
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub struct ReleaseFlags: u8 {
        const NEXT = 1;
    }
}

/// Information about the current and next release.
///
/// The build is set to `-1` if the next release is
/// not available.
#[derive(Clone, Debug)]
pub struct ReleaseInfo {
    pub current: Box<str>,
    pub next: Box<str>,
    pub build: i16,
    pub urgent: Option<u16>,
    pub is_lts: bool,
}

/// Contains information about good and bad repositories.
#[derive(Clone, Debug)]
pub struct RepoCompatError {
    pub success: Vec<String>,
    pub failure: Vec<(String, String)>,
}

/// The status of an action, and a description of why.
#[derive(Clone, Debug)]
pub struct Status {
    pub status: u8,
    pub why: Box<str>,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, FromPrimitive, PartialEq)]
pub enum UpgradeEvent {
    UpdatingPackageLists = 1,
    FetchingPackages = 2,
    UpgradingPackages = 3,
    InstallingPackages = 4,
    UpdatingSourceLists = 5,
    FetchingPackagesForNewRelease = 6,
    AttemptingLiveUpgrade = 7,
    AttemptingSystemdUnit = 8,
    AttemptingRecovery = 9,
    Success = 10,
    SuccessLive = 11,
    Failure = 12,
    AptFilesLocked = 13,
    RemovingConflicts = 14,
    Simulating = 15,
}

impl From<UpgradeEvent> for &'static str {
    fn from(action: UpgradeEvent) -> Self {
        match action {
            UpgradeEvent::AptFilesLocked => "waiting on a process holding the apt lock files",
            UpgradeEvent::AttemptingLiveUpgrade => "attempting live upgrade to the new release",
            UpgradeEvent::AttemptingSystemdUnit => {
                "setting up the system to perform an offline upgrade on the next boot"
            }
            UpgradeEvent::AttemptingRecovery => {
                "setting up the recovery partition to install the new release"
            }
            UpgradeEvent::Failure => "an error occurred while setting up the release upgrade",
            UpgradeEvent::FetchingPackages => "fetching updated packages for the current release",
            UpgradeEvent::FetchingPackagesForNewRelease => "fetching packages for the new release",
            UpgradeEvent::InstallingPackages => {
                "ensuring that system-critical packages are installed"
            }
            UpgradeEvent::RemovingConflicts => "removing deprecated and/or conflicting packages",
            UpgradeEvent::Success => "new release is ready to install",
            UpgradeEvent::SuccessLive => "new release was successfully installed",
            UpgradeEvent::UpdatingPackageLists => "updating package lists",
            UpgradeEvent::UpdatingSourceLists => "updating the source lists",
            UpgradeEvent::UpgradingPackages => "upgrading packages for the current release",
            UpgradeEvent::Simulating => "simulating upgrade",
        }
    }
}

#[derive(Debug, Clone)]
pub enum UpgradeMethod {
    FromFile(PathBuf),
    FromRelease { version: Option<String>, arch: Option<String>, flags: ReleaseFlags },
}
