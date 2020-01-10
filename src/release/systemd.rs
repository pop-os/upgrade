use super::*;
use std::fs;
use ubuntu_version::{Codename, Version};

const PREVIOUS_DEFAULT: &str = "/var/lib/pop-upgrade/previous_default";

pub enum LoaderEntry {
    Current,
    Recovery,
}

/// Defines the specified entry as the default boot entry
pub fn set_default_boot(loader: LoaderEntry) -> RelResult<()> {
    info!("gathering systemd-boot configuration information");

    let mut conf = SystemdBootConf::new("/boot/efi").map_err(ReleaseError::SystemdBootConf)?;

    let comparison: fn(filename: &str) -> bool = match loader {
        LoaderEntry::Current => |e| e.to_lowercase().ends_with("current"),
        LoaderEntry::Recovery => |e| e.to_lowercase().starts_with("recovery"),
    };

    {
        let recovery_entry = conf
            .entries
            .iter()
            .find(|e| comparison(&e.id))
            .ok_or(ReleaseError::MissingRecoveryEntry)?;

        let previous: &str =
            conf.loader_conf.default.as_ref().map(|e| e.as_ref()).unwrap_or_else(|| {
                conf.current_entry().map_or("Pop_OS-current", |e| e.id.as_ref())
            });

        let _ = fs::write(PREVIOUS_DEFAULT, previous);

        conf.loader_conf.default = Some(recovery_entry.id.clone());
    }

    conf.overwrite_loader_conf().map_err(ReleaseError::SystemdBootConfOverwrite)
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