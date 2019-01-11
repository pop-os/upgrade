use ::release_architecture::ReleaseArchError;
use ::release_version::ReleaseVersionError;
use apt_fetcher::{apt_uris::AptUriError, DistUpgradeError};
use std::io;
use async_fetcher::FetchError;
use systemd_boot_conf::Error as SystemdBootConfError;

pub type RelResult<T> = Result<T, ReleaseError>;

#[derive(Debug, Error)]
pub enum ReleaseError {
    #[error(display = "failed to fetch release architecture: {}", _0)]
    ReleaseArch(ReleaseArchError),
    #[error(display = "failed to fetch release versions: {}", _0)]
    ReleaseVersion(ReleaseVersionError),
    #[error(display = "failed to fetch apt URIs to fetch: {}", _0)]
    AptList(AptUriError),
    #[error(display = "unable to upgrade to next release: {}", _0)]
    Check(DistUpgradeError),
    #[error(display = "failure to overwrite release files: {}", _0)]
    Overwrite(DistUpgradeError),
    #[error(display = "root is required for this action: rerun with `sudo`")]
    NotRoot,
    #[error(display = "fetch of package failed: {}", _0)]
    PackageFetch(FetchError),
    #[error(display = "failed to update package lists for the current release: {}", _0)]
    CurrentUpdate(io::Error),
    #[error(display = "failed to update package lists for the new release: {}", _0)]
    ReleaseUpdate(io::Error),
    #[error(display = "failed to perform release upgrade: {}", _0)]
    ReleaseUpgrade(io::Error),
    #[error(display = "failed to perform apt upgrade of the current release: {}", _0)]
    Upgrade(io::Error),
    #[error(display = "failed to install core packages: {}", _0)]
    InstallCore(io::Error),
    #[error(display = "failed to load systemd-boot configuration: {}", _0)]
    SystemdBootConf(SystemdBootConfError),
    #[error(display = "failed to overwrite systemd-boot configuration: {}", _0)]
    SystemdBootConfOverwrite(SystemdBootConfError),
    #[error(display = "recovery entry not found in systemd-boot loader config")]
    MissingRecoveryEntry,
}

impl From<ReleaseVersionError> for ReleaseError {
    fn from(why: ReleaseVersionError) -> Self {
        ReleaseError::ReleaseVersion(why)
    }
}

impl From<ReleaseArchError> for ReleaseError {
    fn from(why: ReleaseArchError) -> Self {
        ReleaseError::ReleaseArch(why)
    }
}
