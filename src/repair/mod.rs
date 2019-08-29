pub mod fstab;
pub mod sources;

use self::{fstab::FstabError, sources::SourcesError};
use ubuntu_version::{Codename, Version, VersionError};

#[derive(Debug, Error)]
pub enum RepairError {
    #[error(display = "error checking and fixing fstab: {}", _0)]
    Fstab(FstabError),
    #[error(display = "version is not an ubuntu codename: {}", _0)]
    InvalidVersion(String),
    #[error(display = "failed to fetch release versions: {}", _0)]
    ReleaseVersion(VersionError),
    #[error(display = "error checkig and fixing sources: {}", _0)]
    Sources(SourcesError),
}

pub fn repair() -> Result<(), RepairError> {
    info!("performing release repair");
    let codename: Codename = Version::detect().map_err(RepairError::ReleaseVersion)?.into();

    fstab::repair().map_err(RepairError::Fstab)?;
    sources::repair(codename).map_err(RepairError::Sources)?;

    Ok(())
}
