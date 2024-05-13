use crate::{
    release_architecture::ReleaseArchError, repair::RepairError, ubuntu_version::VersionError,
};
use std::io;

pub type RelResult<T> = Result<T, ReleaseError>;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReleaseError {
    #[error("failed to fetch apt URIs to fetch")]
    AptList(#[source] anyhow::Error),

    #[error("failed to purge packages")]
    AptPurge(#[source] io::Error),

    #[error("failed to back up system sources")]
    #[allow(clippy::upper_case_acronyms)]
    BackupPPAs(#[source] anyhow::Error),

    #[error("process canceled")]
    Canceled,

    #[error("unable to upgrade to next release: {:?}", _0)]
    Check(#[source] anyhow::Error),

    #[error("failed to launch command")]
    Command(#[source] io::Error),

    #[error("conflicting and/or deprecated packages could not be removed")]
    ConflictRemoval(#[source] anyhow::Error),

    #[error("failed to update package lists for the current release")]
    CurrentUpdate(#[source] io::Error),

    #[error("unable to disable third party repositories")]
    #[allow(clippy::upper_case_acronyms)]
    DisablePPAs(#[source] anyhow::Error),

    #[error("status for `dpkg --configure -a` failed")]
    DpkgConfigure(#[source] io::Error),

    #[error("failed to downgrade packages")]
    Downgrade(#[source] anyhow::Error),

    #[error("status for `apt-get install -f` failed")]
    FixBroken(#[source] io::Error),

    #[error("failed to hold the pop-upgrade package")]
    HoldPopUpgrade(#[source] io::Error),

    #[error("unable to hold apt/dpkg lock files")]
    Lock(#[source] io::Error),

    #[error("root is required for this action: rerun with `sudo`")]
    NotRoot,

    #[error("failed to switch Ubuntu repos to old-releases")]
    OldReleaseSwitch(#[source] io::Error),

    #[error("{:?}", _0)]
    PackageFetch(#[source] anyhow::Error),

    #[error("failed to apply pre-upgrade fixes")]
    PreUpgrade(#[source] RepairError),

    #[error("failed to read the /proc/partitions file")]
    ReadingPartitions(#[source] io::Error),

    #[error("error updating recovery configuration file")]
    RecoveryConf(#[source] anyhow::Error),

    #[error("failed to open the recovery configuration file")]
    RecoveryConfOpen(#[source] io::Error),

    #[error("failed to update the recovery configuration file")]
    RecoveryUpdate(#[source] io::Error),

    #[error("recovery parttiion was not found")]
    RecoveryNotFound,

    #[error("failed to fetch release architecture")]
    ReleaseArch(#[from] ReleaseArchError),

    #[error("failed to update package lists for the new release")]
    ReleaseUpdate(#[source] io::Error),

    #[error("failed to perform release upgrade")]
    ReleaseUpgrade(#[source] io::Error),

    #[error("failed to fetch release versions")]
    ReleaseVersion(#[from] VersionError),

    #[error("failed to apply system repair before upgrade")]
    Repair(#[from] RepairError),

    #[error("failure to simulate upgrade")]
    Simulation(#[source] io::Error),

    #[error("files required for systemd upgrade are missing: {:?}", _0)]
    SystemdUpgradeFilesMissing(Vec<&'static str>),

    #[error("failed to unhold the pop-upgrade package")]
    UnholdPopUpgrade(#[source] io::Error),

    #[error("failed to perform apt upgrade of the current release")]
    Upgrade(#[source] io::Error),

    #[error(
        "unable to install core packages: a package may be preventing pop-desktop from being \
         installed"
    )]
    InstallCore(#[source] io::Error),

    #[error("failed to create /pop-upgrade file")]
    StartupFileCreation(#[source] io::Error),

    #[error("failed to modify systemd-boot configuration: {}", _0)]
    SystemdBoot(anyhow::Error),

    #[error(
        "attempted recovery-based upgrade method, but the systemd efi loader path was not found"
    )]
    SystemdBootEfiPathNotFound,

    #[error("attempted recovery-based upgrade method, but the systemd boot loader was not found")]
    SystemdBootLoaderNotFound,

    #[error("failed to get transitional snap packages")]
    TransitionalSnapFetch(#[source] anyhow::Error),

    #[error("failed to hold transitional snap package")]
    TransitionalSnapHold(#[source] io::Error),

    #[error("failed to record held transitional snap packages")]
    TransitionalSnapRecord(#[source] io::Error),

    #[error("recovery entry not found in systemd-boot loader config")]
    MissingRecoveryEntry,
}
