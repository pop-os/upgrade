use crate::{
    checksum::ValidateError, release_api::ApiError, release_architecture::ReleaseArchError,
    repair::RepairError,
};

use std::{io, path::PathBuf};
use thiserror::Error;
use ubuntu_version::VersionError;

pub type RecResult<T> = Result<T, RecoveryError>;

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error("failed to fetch release data from server")]
    ApiError(#[from] ApiError),

    #[error("process has been cancelled")]
    Cancelled,

    #[error("checksum for {:?} failed: {}", path, source)]
    Checksum { path: PathBuf, source: ValidateError },

    #[error("failed to download ISO")]
    Download(#[source] Box<RecoveryError>),

    #[error("fetching from {} failed: {}", url, source)]
    Fetch { url: String, source: anyhow::Error },

    #[error("generic I/O error")]
    Io(#[from] io::Error),

    #[error("ISO does not exist at path")]
    IsoNotFound,

    #[error("failed to fetch mount points")]
    Mounts(#[source] io::Error),

    #[error("no build was found to fetch")]
    NoBuildAvailable,

    #[error("failed to create temporary directory for ISO")]
    TempDir(#[source] io::Error),

    #[error("recovery partition was not found")]
    RecoveryNotFound,

    #[error("failed to apply system repair before recovery upgrade")]
    Repair(#[from] RepairError),

    #[error("EFI partition was not found")]
    EfiNotFound,

    #[error("failed to fetch release architecture")]
    ReleaseArch(#[from] ReleaseArchError),

    #[error("failed to fetch release versions")]
    ReleaseVersion(#[from] VersionError),

    #[error("the recovery feature is limited to EFI installs")]
    Unsupported,

    #[error("failed to write version of ISO now stored on the recovery partition")]
    WriteVersion(#[source] io::Error),
}
