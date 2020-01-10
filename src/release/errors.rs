use crate::{release_architecture::ReleaseArchError, repair::RepairError};
use apt_fetcher::{apt_uris::AptUriError, DistUpgradeError};
use async_fetcher::FetchError;
use std::io;
use ubuntu_version::VersionError;

pub type RelResult<T> = Result<T, ReleaseError>;

#[derive(Debug, Error)]
pub enum ReleaseError {
    #[error(display = "failed to fetch apt URIs to fetch: {}", _0)]
    AptList(AptUriError),
    #[error(display = "failed to purge packages: {}", _0)]
    AptPurge(io::Error),
    #[error(display = "unable to upgrade to next release: {}", _0)]
    Check(DistUpgradeError),
    #[error(display = "conflicting and/or deprecated packages could not be removed")]
    ConflictRemoval(#[error(source, no_from)] io::Error),
    #[error(display = "failed to update package lists for the current release: {}", _0)]
    CurrentUpdate(io::Error),
    #[error(display = "status for `dpkg --configure -a` failed: {}", _0)]
    DpkgConfigure(io::Error),
    #[error(display = "status for `apt-get install -f` failed: {}", _0)]
    FixBroken(io::Error),
    #[error(display = "failed to hold the pop-upgrade package: {}", _0)]
    HoldPopUpgrade(io::Error),
    #[error(display = "unable to hold apt/dpkg lock files: {}", _0)]
    Lock(io::Error),
    #[error(display = "failure to overwrite release files: {}", _0)]
    Overwrite(DistUpgradeError),
    #[error(display = "root is required for this action: rerun with `sudo`")]
    NotRoot,
    #[error(display = "fetch of package '{}' at {} failed: {}", _0, _1, _2)]
    PackageFetch(String, String, FetchError),
    #[error(display = "failed to apply pre-upgrade fixes: {}", _0)]
    PreUpgrade(RepairError),
    #[error(display = "failed to read the /proc/partitions file: {}", _0)]
    ReadingPartitions(io::Error),
    #[error(display = "failed to open the recovery configuration file: {}", _0)]
    RecoveryConfOpen(io::Error),
    #[error(display = "failed to update the recovery configuration file: {}", _0)]
    RecoveryUpdate(io::Error),
    #[error(display = "recovery parttiion was not found")]
    RecoveryNotFound,
    #[error(display = "failed to fetch release architecture: {}", _0)]
    ReleaseArch(ReleaseArchError),
    #[error(display = "failed to create release fetch file: {}", _0)]
    ReleaseFetchFile(io::Error),
    #[error(display = "failed to update package lists for the new release: {}", _0)]
    ReleaseUpdate(io::Error),
    #[error(display = "failed to perform release upgrade: {}", _0)]
    ReleaseUpgrade(io::Error),
    #[error(display = "failed to fetch release versions: {}", _0)]
    ReleaseVersion(VersionError),
    #[error(display = "failed to apply system repair before upgrade: {}", _0)]
    Repair(RepairError),
    #[error(display = "failed to modify systemd-boot configuration: {}", _0)]
    SystemdBoot(anyhow::Error),
    #[error(display = "files required for systemd upgrade are missing: {:?}", _0)]
    SystemdUpgradeFilesMissing(Vec<&'static str>),
    #[error(display = "failed to unhold the pop-upgrade package: {}", _0)]
    UnholdPopUpgrade(io::Error),
    #[error(display = "failed to perform apt upgrade of the current release: {}", _0)]
    Upgrade(io::Error),
    #[error(display = "unable to install core packages: a package may be preventing pop-desktop \
                       from being installed")]
    InstallCore(#[error(source, no_from)] io::Error),
    #[error(display = "failed to create /pop-upgrade file: {}", _0)]
    StartupFileCreation(io::Error),
    #[error(display = "attempted recovery-based upgrade method, but the systemd efi loader path \
                       was not found")]
    SystemdBootEfiPathNotFound,
    #[error(display = "attempted recovery-based upgrade method, but the systemd boot loader was \
                       not found")]
    SystemdBootLoaderNotFound,
    #[error(display = "failed to get transitional snap packages: {}", _0)]
    TransitionalSnapFetch(io::Error),
    #[error(display = "failed to hold transitional snap package: {}", _0)]
    TransitionalSnapHold(io::Error),
    #[error(display = "failed to record held transitional snap packages: {}", _0)]
    TransitionalSnapRecord(io::Error),
    #[error(display = "recovery entry not found in systemd-boot loader config")]
    MissingRecoveryEntry,
}

impl From<VersionError> for ReleaseError {
    fn from(why: VersionError) -> Self { ReleaseError::ReleaseVersion(why) }
}

impl From<ReleaseArchError> for ReleaseError {
    fn from(why: ReleaseArchError) -> Self { ReleaseError::ReleaseArch(why) }
}
