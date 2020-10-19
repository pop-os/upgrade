pub mod fstab;
pub mod misc;
pub mod packaging;

use self::fstab::FstabError;
use std::io;

#[derive(Debug, Error)]
pub enum RepairError {
    #[error(display = "packaging error: {}", _0)]
    Packaging(anyhow::Error),
    #[error(display = "error checking and fixing fstab: {}", _0)]
    Fstab(FstabError),
    #[error(display = "version is not an ubuntu codename: {}", _0)]
    InvalidVersion(String),
    #[error(display = "unable to apply dkms gcc9 fix: {}", _0)]
    DkmsGcc9(io::Error),
    #[error(display = "failed to wipe pulseaudio settings for users: {}", _0)]
    WipePulse(io::Error),
}

pub fn repair() -> Result<(), RepairError> {
    info!("performing release repair");
    fstab::repair().map_err(RepairError::Fstab)?;
    packaging::repair().map_err(RepairError::Packaging)?;

    Ok(())
}

pub fn pre_upgrade() -> Result<(), RepairError> {
    misc::dkms_gcc9_fix().map_err(RepairError::DkmsGcc9)?;
    misc::wipe_pulse().map_err(RepairError::WipePulse)
}
