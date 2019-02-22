pub mod fstab;
pub mod sources;

use self::fstab::FstabError;
use self::sources::SourcesError;

#[derive(Debug, Error)]
pub enum RepairError {
    #[error(display = "error checking and fixing fstab: {}", _0)]
    Fstab(FstabError),
    #[error(display = "error checkig and fixing sources: {}", _0)]
    Sources(SourcesError),
}

pub fn repair(current_release: &str) -> Result<(), RepairError> {
    fstab::repair().map_err(RepairError::Fstab)?;
    sources::repair(current_release).map_err(RepairError::Sources)?;

    Ok(())
}
