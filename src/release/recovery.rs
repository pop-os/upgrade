use super::*;

use anyhow::Context;
use envfile::EnvFile;
use std::path::Path;

/// Checks if the `MODE` in `/recovery/recovery.conf` is set to the given option.
pub fn mode_is(option: &str) -> anyhow::Result<bool> {
    open().map(|env| env.get("MODE").map_or(false, |mode| mode == option))
}

/// Fetch the recovery configuration, and designate the recovery partition as the default
/// boot option.
///
/// It will be up to the recovery partition to revert this change once it has completed its job.
pub fn mode_set(mode: &str, prev_boot: &str) -> anyhow::Result<()> {
    open()?
        .update("MODE", mode)
        .update("PREV_BOOT", prev_boot)
        .write()
        .context("failed to update the recovery configuration file")
}

/// Unsets the `MODE` variable defined in `/recovery/recovery.conf`.
pub fn mode_unset() -> anyhow::Result<()> {
    let mut envfile = open()?;
    envfile.store.remove("MODE");
    envfile.store.remove("PREV_BOOT");
    envfile.write().context("failed to update the recovery configuration file")
}

/// Opens the recovery configuration file
pub fn open() -> anyhow::Result<EnvFile> {
    EnvFile::new(Path::new("/recovery/recovery.conf"))
        .context("failed to open the recovery configuration file")
}

/// Checks if necessary requirements to use the recovery partition are made.
pub fn upgrade_prereq() -> anyhow::Result<()> {
    if !Path::new(SYSTEMD_BOOT_LOADER).exists() {
        return Err(anyhow!(
            "attempted recovery-based upgrade method, but the systemd boot loader was not found"
        ));
    }

    if !Path::new(SYSTEMD_BOOT_LOADER_PATH).exists() {
        return Err(anyhow!(
            "attempted recovery-based upgrade method, but the systemd boot efi loader path was \
             not found"
        ));
    }

    let partitions =
        fs::read_to_string("/proc/mounts").context("failed to fetch list of partitions")?;

    if partitions.contains("/recovery") {
        Ok(())
    } else {
        Err(anyhow!("recovery partition was not found"))
    }
}
