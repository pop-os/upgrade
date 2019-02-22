pub mod fstab;
pub mod sources;

use self::fstab::FstabError;
use self::sources::SourcesError;
use crate::release_version::{detect_version, ReleaseVersionError};
use crate::ubuntu_codename::UbuntuCodename;

#[derive(Debug, Error)]
pub enum RepairError {
    #[error(display = "error checking and fixing fstab: {}", _0)]
    Fstab(FstabError),
    #[error(display = "version is not an ubuntu codename: {}", _0)]
    InvalidVersion(String),
    #[error(display = "failed to fetch release versions: {}", _0)]
    ReleaseVersion(ReleaseVersionError),
    #[error(display = "error checkig and fixing sources: {}", _0)]
    Sources(SourcesError),
}

pub fn repair() -> Result<(), RepairError> {
    let (current, _) = detect_version().map_err(RepairError::ReleaseVersion)?;
    let codename = UbuntuCodename::from_version(&current)
        .ok_or_else(move || RepairError::InvalidVersion(current))?
        .into_codename();

    fstab::repair().map_err(RepairError::Fstab)?;
    sources::repair(codename).map_err(RepairError::Sources)?;

    Ok(())
}
