use super::*;

use anyhow::Context;
use std::fs;
use ubuntu_version::{Codename, Version};

pub const PREVIOUS_DEFAULT: &str = "/var/lib/pop-upgrade/previous_default";

pub enum LoaderEntry {
    Current,
    Recovery,
}

/// Restores the previous default boot entry
pub fn restore_default() -> anyhow::Result<()> {
    if !Path::new(PREVIOUS_DEFAULT).exists() {
        return Ok(());
    }

    fs::read_to_string(PREVIOUS_DEFAULT)
        .context("failed to read previous default boot entry")
        .and_then(|entry| set_default_boot_id(&entry))
        .and_then(|_| remove_previous())
}

/// Modified the default boot entry
pub fn set_default_boot<F: Fn(&mut SystemdBootConf) -> anyhow::Result<()>>(
    modify: F,
) -> anyhow::Result<()> {
    info!("gathering systemd-boot configuration information");

    const DEFAULT_BOOT: &str = "Pop_OS-current";

    let mut conf =
        SystemdBootConf::new("/boot/efi").context("failed to load systemd-boot configuration")?;

    let mut previous: &str = conf
        .loader_conf
        .default
        .as_ref()
        .map(Box::as_ref)
        .unwrap_or_else(|| conf.current_entry().map_or(DEFAULT_BOOT, |e| e.id.as_ref()));

    if previous.starts_with("Recovery") {
        previous = DEFAULT_BOOT;
    }

    let _ = fs::write(PREVIOUS_DEFAULT, previous);

    modify(&mut conf)?;

    conf.overwrite_loader_conf().context("failed to overwrite systemd-boot configuration")
}

/// Defines the specified entry as the default boot entry
pub fn set_default_boot_id(id: &str) -> anyhow::Result<()> {
    set_default_boot(|conf| {
        conf.loader_conf.default = Some(id.into());
        Ok(())
    })
}

/// Defines the specified entry as the default boot entry
pub fn set_default_boot_variant(variant: LoaderEntry) -> anyhow::Result<()> {
    set_default_boot(|conf| {
        let comparison: fn(filename: &str) -> bool = match variant {
            LoaderEntry::Current => |e| e.to_lowercase().ends_with("current"),
            LoaderEntry::Recovery => |e| e.to_lowercase().starts_with("recovery"),
        };

        let recovery_entry = conf
            .entries
            .iter()
            .find(|e| comparison(&e.id))
            .ok_or(ReleaseError::MissingRecoveryEntry)?;

        conf.loader_conf.default = Some(recovery_entry.id.clone());
        Ok(())
    })
}

/// Create the system upgrade files that systemd will check for at startup.
pub fn upgrade_set(from: &str, to: &str) -> RelResult<()> {
    let current = from
        .parse::<Version>()
        .ok()
        .and_then(|x| Codename::try_from(x).ok())
        .map(<&'static str>::from)
        .unwrap_or(from);

    let new = to
        .parse::<Version>()
        .ok()
        .and_then(|x| Codename::try_from(x).ok())
        .map(<&'static str>::from)
        .unwrap_or(to);

    fs::write(STARTUP_UPGRADE_FILE, &format!("{} {}", current, new))
        .and_then(|_| symlink("/var/cache/apt/archives", SYSTEM_UPDATE))
        .map_err(ReleaseError::StartupFileCreation)
}

/// Validate that the pre-required files for performing a system upgrade are in place.
pub fn upgrade_prereq() -> RelResult<()> {
    const REQUIRED_UPGRADE_FILES: [&str; 3] = [
        "/usr/lib/pop-upgrade/upgrade.sh",
        "/lib/systemd/system/pop-upgrade-init.service",
        "/lib/systemd/system/system-update.target.wants/pop-upgrade-init.service",
    ];

    let invalid = REQUIRED_UPGRADE_FILES
        .iter()
        .cloned()
        .filter(|file| !Path::new(file).is_file())
        .collect::<Vec<&'static str>>();

    if !invalid.is_empty() {
        return Err(ReleaseError::SystemdUpgradeFilesMissing(invalid));
    }

    Ok(())
}

fn remove_previous() -> anyhow::Result<()> {
    fs::remove_file(PREVIOUS_DEFAULT).with_context(|| fomat!("failed to remove "(PREVIOUS_DEFAULT)))
}
