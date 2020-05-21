pub mod fstab;
pub mod misc;
pub mod packaging;
pub mod sources;

use self::{fstab::FstabError, sources::SourcesError};
use std::{convert::TryFrom, io};
use ubuntu_version::{Codename, Version, VersionError};

#[derive(Debug, Error)]
pub enum RepairError {
    #[error(display = "packaging error: {}", _0)]
    Packaging(anyhow::Error),
    #[error(display = "error checking and fixing fstab: {}", _0)]
    Fstab(FstabError),
    #[error(display = "version is not an ubuntu codename: {}", _0)]
    InvalidVersion(String),
    #[error(display = "failed to fetch release versions: {}", _0)]
    ReleaseVersion(VersionError),
    #[error(display = "error checking and fixing sources: {}", _0)]
    Sources(SourcesError),
    #[error(display = "unable to apply dkms gcc9 fix: {}", _0)]
    DkmsGcc9(io::Error),
    #[error(display = "unknown Ubuntu release: {}", _0)]
    UnknownRelease(Version),
    #[error(display = "failed to wipe pulseaudio settings for users: {}", _0)]
    WipePulse(io::Error),
}

pub fn repair() -> Result<(), RepairError> {
    info!("performing release repair");
    let version = Version::detect().map_err(RepairError::ReleaseVersion)?;

    let codename = Codename::try_from(version).map_err(|_| RepairError::UnknownRelease(version))?;

    fstab::repair().map_err(RepairError::Fstab)?;
    sources::repair(codename).map_err(RepairError::Sources)?;
    packaging::repair(<&'static str>::from(codename)).map_err(RepairError::Packaging)?;

    Ok(())
}

pub fn pre_upgrade() -> Result<(), RepairError> {
    misc::dkms_gcc9_fix().map_err(RepairError::DkmsGcc9)?;
    misc::wipe_pulse().map_err(RepairError::WipePulse)
}
