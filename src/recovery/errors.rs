use crate::{
    checksum::ValidateError, release_api::ApiError, release_architecture::ReleaseArchError,
    repair::RepairError,
};
use ubuntu_version::VersionError;

use std::{io, path::PathBuf};

pub type RecResult<T> = Result<T, RecoveryError>;

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error(display = "failed to fetch release data from server: {}", _0)]
    ApiError(ApiError),
    #[error(display = "checksum for {:?} failed: {}", path, why)]
    Checksum { path: PathBuf, why: ValidateError },
    #[error(display = "failed to download ISO: {}", _0)]
    Download(Box<RecoveryError>),
    #[error(display = "fetching from {} failed: {}", url, why)]
    Fetch { url: String, why: io::Error },
    #[error(display = "I/O error: {}", _0)]
    Io(io::Error),
    #[error(display = "ISO does not exist at path")]
    IsoNotFound,
    #[error(display = "no build was found to fetch")]
    NoBuildAvailable,
    #[error(display = "failed to create temporary directory for ISO: {}", _0)]
    TempDir(io::Error),
    #[error(display = "recovery partition was not found")]
    RecoveryNotFound,
    #[error(display = "failed to apply system repair before recovery upgrade: {}", _0)]
    Repair(RepairError),
    #[error(display = "EFI partition was not found")]
    EfiNotFound,
    #[error(display = "failed to fetch release architecture: {}", _0)]
    ReleaseArch(ReleaseArchError),
    #[error(display = "failed to fetch release versions: {}", _0)]
    ReleaseVersion(VersionError),
    #[error(display = "the recovery feature is limited to EFI installs")]
    Unsupported,
    #[error(
        display = "failed to write version of ISO now stored on the recovery partition: {}",
        _0
    )]
    WriteVersion(io::Error),
}

impl From<io::Error> for RecoveryError {
    fn from(why: io::Error) -> Self { RecoveryError::Io(why) }
}

impl From<VersionError> for RecoveryError {
    fn from(why: VersionError) -> Self { RecoveryError::ReleaseVersion(why) }
}

impl From<ReleaseArchError> for RecoveryError {
    fn from(why: ReleaseArchError) -> Self { RecoveryError::ReleaseArch(why) }
}
