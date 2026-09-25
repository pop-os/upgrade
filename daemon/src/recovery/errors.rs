use crate::release::ReleaseCheckError;
use crate::release_api::ApiError;
use crate::release_architecture::ReleaseArchError;
use crate::repair::RepairError;
use crate::ubuntu_version::VersionError;
use std::io;
use std::path::PathBuf;

pub type RecResult<T> = Result<T, RecoveryError>;

error_set::error_set! {
    RecoveryError := {
        #[display("failed to fetch release data from server")]
        Apidisplay(ApiError),
        #[display("process has been cancelled")]
        Cancelled,
        #[display("checksum for {:?} failed", path)]
        Checksum(async_fetcher::ChecksumError) { path: PathBuf },
        #[display("checksum is not SHA256: {}", checksum)]
        ChecksumInvalid(hex::FromHexError) { checksum: String },
        #[display("failed to copy ISO contents to /recovery")]
        Copy(io::Error),
        #[display("failed to copy kernel from ISO to EFI partition")]
        CopyKernel(io::Error),
        #[display("failed to create /recovery directory")]
        CreateRecoveryDir(io::Error),
        #[display("fetching from {} failed", url)]
        Fetch(async_fetcher::Error) { url: String },
        #[display("cannot find UUID of recovery partition")]
        FindUuid,
        #[display("ISO does not exist at path")]
        IsoNotFound,
        #[display("failed to mount recovery ISO")]
        MountIso(io::Error),
        #[display("failed to fetch mount points")]
        Mounts(io::Error),
        #[display("no release ISO was found")]
        NoBuildAvailable,
        #[display("failed to create temporary directory for ISO")]
        TempDir(io::Error),
        #[display("recovery partition was not found")]
        RecoveryNotFound,
        #[display("no release ISO found for {version}")]
        ReleaseCheck(ApiError) { version: String },
        #[display("no ISO found for current release")]
        ReleaseCheckCurrent(ReleaseCheckError),
        #[display("`pop-upgrade release repair` returned an error")]
        Repair(RepairError),
        #[display("EFI partition was not found")]
        EfiNotFound,
        #[display("failed to fetch release architecture")]
        ReleaseArch(ReleaseArchError),
        #[display("failed to fetch release versions")]
        ReleaseVersion(VersionError),
        #[display("failed to get status of recovery fetch task")]
        TokioJoin(tokio::task::JoinError),
        #[display("the recovery feature is limited to EFI installs")]
        Unsupported,
        #[display("failed to write version of ISO now stored on the recovery partition")]
        WriteVersion(io::Error),
    }
}
