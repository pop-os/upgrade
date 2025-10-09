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
/// - Ensures that `/boot/efi` and `/` are mounted.
pub fn repair() -> Result<(), FstabError> {
    if SystemEnvironment::detect() != SystemEnvironment::Efi {
        return Ok(());
    }

    // Ensure that all devices have been mounted before proceeding.
    mount_required_partitions().map_err(FstabError::MountFailure)
}

/// Ensure that the necessary mount points are mounted.
fn mount_required_partitions() -> anyhow::Result<()> {
    // Check /proc/mounts for existing mountpoints rather than relying entirely
    // on mount(1), which gets confused by ZFS filesystems (which might be
    // managed by e.g. zfs-mount.service).
    let mounts = proc_mounts::MountList::new().context("failed to read /proc/mounts")?;

    for mount_point in &["/", "/boot/efi"] {
        if let Some(_) = mounts.get_mount_by_dest(mount_point) {
            continue; // Already mounted.
        }
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
