pub mod crypttab;
pub mod fstab;
pub mod misc;
pub mod packaging;

use self::fstab::FstabError;
use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RepairError {
    #[error("failed to correct errors in crypttab")]
    Crypttab(#[source] anyhow::Error),

    #[error("unable to apply dkms gcc9 fix")]
    DkmsGcc9(#[source] io::Error),

    #[error("error checking and fixing fstab")]
    Fstab(#[source] FstabError),

    #[error("version is not an ubuntu codename: {}", _0)]
    InvalidVersion(String),

    #[error("packaging error")]
    Packaging(#[source] anyhow::Error),

    #[error("failed to wipe pulseaudio settings for users")]
    WipePulse(#[source] io::Error),
}

pub async fn repair() -> Result<(), RepairError> {
    log::info!("performing release repair");

    crypttab::repair().map_err(RepairError::Crypttab)?;
    fstab::repair().map_err(RepairError::Fstab)?;
    packaging::repair().await.map_err(RepairError::Packaging)?;

    Ok(())
}

pub fn pre_upgrade() -> Result<(), RepairError> {
    misc::dkms_gcc9_fix().map_err(RepairError::DkmsGcc9)?;
    misc::wipe_pulse().map_err(RepairError::WipePulse)
}
