use crate::release::errors::RecoveryConfError;
use crate::release::{ReleaseError, SYSTEMD_BOOT_LOADER, SYSTEMD_BOOT_LOADER_PATH};
use envfile::EnvFile;
use std::path::Path;

/// Checks if the `MODE` in `/recovery/recovery.conf` is set to the given option.
pub fn mode_is(option: &str) -> Result<bool, RecoveryConfError> {
    Ok(EnvFile::new(Path::new("/recovery/recovery.conf"))
        .map_err(RecoveryConfError::ConfOpen)?
        .get("MODE")
        .map_or(false, |mode| mode == option))
}

/// Fetch the systemd-boot configuration, and designate the recovery partition as the default
/// boot option.
///
/// It will be up to the recovery partition to revert this change once it has completed its job.
pub fn mode_set(mode: &str, prev_boot: &str) -> Result<(), RecoveryConfError> {
    EnvFile::new(Path::new("/recovery/recovery.conf"))
        .map_err(RecoveryConfError::ConfOpen)?
        .update("MODE", mode)
        .update("PREV_BOOT", prev_boot)
        .write()
        .map_err(RecoveryConfError::ConfUpdate)
}

/// Unsets the `MODE` variable defined in `/recovery/recovery.conf`.
pub fn mode_unset() -> Result<(), RecoveryConfError> {
    let mut envfile =
        EnvFile::new(Path::new("/recovery/recovery.conf")).map_err(RecoveryConfError::ConfOpen)?;

    envfile.store.remove("MODE");
    envfile.store.remove("PREV_BOOT");

    envfile.write().map_err(RecoveryConfError::ConfUpdate)
}

/// Checks if necessary requirements to use the recovery partition are made.
pub fn upgrade_prereq() -> Result<(), ReleaseError> {
    if !Path::new(SYSTEMD_BOOT_LOADER).exists() {
        return Err(ReleaseError::SystemdBootLoaderNotFound);
    }

    if !Path::new(SYSTEMD_BOOT_LOADER_PATH).exists() {
        return Err(ReleaseError::SystemdBootEfiPathNotFound);
    }

    let partitions =
        std::fs::read_to_string("/proc/mounts").map_err(ReleaseError::ReadingPartitions)?;

    if partitions.contains("/recovery") {
        Ok(())
    } else {
        Err(ReleaseError::RecoveryNotFound)
    }
}
