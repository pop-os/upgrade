use super::*;

use envfile::EnvFile;
use std::path::Path;

/// Checks if the `MODE` in `/recovery/recovery.conf` is set to the given option.
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

/// Unsets the `MODE` variable defined in `/recovery/recovery.conf`.
pub fn mode_unset() -> RelResult<()> {
    let mut envfile = EnvFile::new(Path::new("/recovery/recovery.conf"))
        .map_err(ReleaseError::RecoveryConfOpen)?;

    envfile.store.remove("MODE");

    envfile.write().map_err(ReleaseError::RecoveryUpdate)
}

/// Checks if necessary requirements to use the recovery partition are made.
pub fn upgrade_prereq() -> RelResult<()> {
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
