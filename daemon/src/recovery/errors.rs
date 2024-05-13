use crate::{
    release_api::ApiError, release_architecture::ReleaseArchError, repair::RepairError,
    ubuntu_version::VersionError,
};
use std::{io, path::PathBuf};
use thiserror::Error;

pub type RecResult<T> = Result<T, RecoveryError>;

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error("failed to fetch release data from server")]
    ApiError(#[from] ApiError),

    #[error("{:?}", _0)]
    Anyhow(#[from] anyhow::Error),

    #[error("process has been cancelled")]
    Cancelled,

    #[error("checksum for {:?} failed: {}", path, source)]
    Checksum { path: PathBuf, source: async_fetcher::ChecksumError },

    #[error("checksum is not SHA256: {}", checksum)]
    ChecksumInvalid { checksum: String, source: hex::FromHexError },

    #[error("fetching from {} failed: {}", url, source)]
    Fetch { url: String, source: async_fetcher::Error },

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

    #[error("failed to get status of recovery fetch task")]
    TokioJoin(#[from] tokio::task::JoinError),

    #[error("the recovery feature is limited to EFI installs")]
    Unsupported,

    #[error("failed to write version of ISO now stored on the recovery partition")]
    WriteVersion(#[source] io::Error),
}
