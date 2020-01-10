use super::*;

use envfile::EnvFile;
use std::path::Path;

pub fn mode_is(option: &str) -> RelResult<bool> {
    Ok(EnvFile::new(Path::new("/recovery/recovery.conf"))
        .map_err(ReleaseError::RecoveryConfOpen)?
        .get("MODE")
        .map_or(false, |mode| mode == option))
}

/// Fetch the systemd-boot configuration, and designate the recovery partition as the default
/// boot option.
///
/// It will be up to the recovery partition to revert this change once it has completed its job.
pub fn mode_set(mode: &str) -> RelResult<()> {
    EnvFile::new(Path::new("/recovery/recovery.conf"))
        .map_err(ReleaseError::RecoveryConfOpen)?
        .update("MODE", mode)
        .write()
        .map_err(ReleaseError::RecoveryUpdate)
}

pub fn mode_unset() -> RelResult<()> {
    let mut envfile = EnvFile::new(Path::new("/recovery/recovery.conf"))
        .map_err(ReleaseError::RecoveryConfOpen)?;

    envfile.store.remove("MODE");

    envfile.write().map_err(ReleaseError::RecoveryUpdate)
}

pub fn prereq() -> RelResult<()> {
    if !Path::new(SYSTEMD_BOOT_LOADER).exists() {
        return Err(ReleaseError::SystemdBootLoaderNotFound);
    }

    if !Path::new(SYSTEMD_BOOT_LOADER_PATH).exists() {
        return Err(ReleaseError::SystemdBootEfiPathNotFound);
    }

    let partitions = fs::read_to_string("/proc/mounts").map_err(ReleaseError::ReadingPartitions)?;

    if partitions.contains("/recovery") {
        Ok(())
    } else {
        Err(ReleaseError::RecoveryNotFound)
    }
}
