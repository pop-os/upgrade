use crate::{release_api::ApiError, release_architecture::ReleaseArchError, repair::RepairError};

use async_fetcher_preview::ChecksummerError;
use std::io;
use thiserror::Error;
use ubuntu_version::VersionError;

pub type RecResult<T> = Result<T, RecoveryError>;

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error("failed to fetch release data from server: {}", _0)]
    ApiError(ApiError),
    #[error("process has been cancelled")]
    Cancelled,
    #[error("Release API did not provide a valid SHA256 string")]
    ChecksumString(#[from] hex::FromHexError),
    #[error("mismatch in checksum of fetched ISO")]
    ChecksumValidate(#[from] ChecksummerError),
    #[error("failed to download ISO: {}", _0)]
    Download(Box<RecoveryError>),
    #[error("fetching from {} failed: {}", url, why)]
    Fetch { url: String, why: async_fetcher_preview::Error },
    #[error("generic I/O error: {}", _0)]
    Io(io::Error),
    #[error("ISO does not exist at path")]
    IsoNotFound,
    #[error("failed to fetch mount points")]
    Mounts(#[source] io::Error),
    #[error("no build was found to fetch")]
    NoBuildAvailable,
    #[error("failed to create temporary directory for ISO: {}", _0)]
    TempDir(io::Error),
    #[error("recovery partition was not found")]
    RecoveryNotFound,
    #[error("failed to apply system repair before recovery upgrade: {}", _0)]
    Repair(RepairError),
    #[error("EFI partition was not found")]
    EfiNotFound,
    #[error("failed to fetch release architecture: {}", _0)]
    ReleaseArch(#[from] ReleaseArchError),
    #[error("failed to fetch release versions: {}", _0)]
    ReleaseVersion(#[from] VersionError),
    #[error("the recovery feature is limited to EFI installs")]
    Unsupported,
    #[error("failed to write version of ISO now stored on the recovery partition: {}", _0)]
    WriteVersion(io::Error),
}

impl From<io::Error> for RecoveryError {
    fn from(why: io::Error) -> Self { RecoveryError::Io(why) }
}
