use ::checksum::ValidateError;
use ::release_api::ApiError;
use ::release_architecture::ReleaseArchError;
use ::release_version::ReleaseVersionError;

use std::io;
use std::path::PathBuf;

pub type RecResult<T> = Result<T, RecoveryError>;

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error(display = "ISO does not exist at path")]
    IsoNotFound,
    #[error(display = "failed to fetch release data from server: {}", _0)]
    ApiError(ApiError),
    #[error(display = "failed to download ISO: {}", _0)]
    Download(Box<RecoveryError>),
    #[error(display = "failed to create temporary directory for ISO: {}", _0)]
    TempDir(io::Error),
    #[error(display = "I/O error: {}", _0)]
    Io(io::Error),
    #[error(display = "fetching from {} failed: {}", url, why)]
    Fetch { url: String, why: io::Error },
    #[error(display = "checksum for {:?} failed: {}", path, why)]
    Checksum { path: PathBuf, why: ValidateError },
    #[error(display = "recovery partition was not found")]
    RecoveryNotFound,
    #[error(display = "EFI partition was not found")]
    EfiNotFound,
    #[error(display = "failed to probe for recovery partition: {}", _0)]
    Probe(io::Error),
    #[error(display = "failed to fetch release architecture: {}", _0)]
    ReleaseArch(ReleaseArchError),
    #[error(display = "failed to fetch release versions: {}", _0)]
    ReleaseVersion(ReleaseVersionError)
}

impl From<io::Error> for RecoveryError {
    fn from(why: io::Error) -> Self {
        RecoveryError::Io(why)
    }
}

impl From<ReleaseVersionError> for RecoveryError {
    fn from(why: ReleaseVersionError) -> Self {
        RecoveryError::ReleaseVersion(why)
    }
}

impl From<ReleaseArchError> for RecoveryError {
    fn from(why: ReleaseArchError) -> Self {
        RecoveryError::ReleaseArch(why)
    }
}
