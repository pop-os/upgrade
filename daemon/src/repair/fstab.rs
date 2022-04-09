//! All code responsible for validating and repair the /etc/fstab file.

use crate::system_environment::SystemEnvironment;
use anyhow::Context;
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FstabError {
    #[error("failed to mount devices with `mount -a`")]
    MountFailure(#[source] anyhow::Error),
}

/// Performs the following Pop-specific actions:
///
/// - Ensures that `/boot/efi` and `/recovery` are mounted by PartUUID.
pub fn repair() -> Result<(), FstabError> {
    if SystemEnvironment::detect() != SystemEnvironment::Efi {
        return Ok(());
    }

    // Ensure that all devices have been mounted before proceeding.
    mount_required_partitions().map_err(FstabError::MountFailure)
}

/// Ensure that the necessary mount points are mounted.
fn mount_required_partitions() -> anyhow::Result<()> {
    for mount_point in &["/", "/boot/efi"] {
        Command::new("mount")
            .arg(mount_point)
            .status()
            .context("failed to spawn mount command")
            .and_then(|status| {
                // 0 means it mounted an unmounted drive.
                // 32 means it was already mounted.
                match status.code() {
                    Some(0) | Some(32) => Ok(()),
                    _ => Err(anyhow!("failed to mount `{}` partition", mount_point)),
                }
            })?;
    }

    Ok(())
}
