use super::*;

use crate::ubuntu_version::{Codename, Version};
use std::{fs, io};

pub const PREVIOUS_DEFAULT: &str = "/var/lib/pop-upgrade/previous_default";

error_set::error_set! {
    Error := {
        #[display("failed to overwite default boot entry")]
        DefaultBootSet(systemd_boot_conf::Error),
        #[display("failed to load systemd-boot loader config")]
        Load(systemd_boot_conf::Error),
        #[display("recovery entry not found in systemd-boot loader config")]
        MissingRecoveryEntry,
        #[display("failed to read previous default boot entry")]
        PreviousBootRead(io::Error),
        #[display("failed to remove {}", PREVIOUS_DEFAULT)]
        RemovePreviousDefault(io::Error),
    }
}

pub struct BootConf(pub SystemdBootConf);

impl BootConf {
    const DEFAULT_BOOT: &'static str = "Pop_OS-current";

    pub fn load() -> Result<Self, Error> {
        SystemdBootConf::new("/boot/efi")
            .map_err(Error::Load)
            .map(Self)
    }

    pub fn default_boot(&self) -> &str {
        self.0
            .loader_conf
            .default
            .as_ref()
            .map(Box::as_ref)
            .unwrap_or_else(|| {
                self.0
                    .current_entry()
                    .map_or(Self::DEFAULT_BOOT, |e| e.id.as_ref())
            })
    }

    /// Modified the default boot entry
    pub fn set_default_boot<F: Fn(&mut SystemdBootConf) -> Result<(), Error>>(
        &mut self,
        modify: F,
    ) -> Result<(), Error> {
        info!("gathering systemd-boot configuration information");

        let mut previous = self.default_boot();

        if previous.starts_with("Recovery") {
            previous = Self::DEFAULT_BOOT;
        }

        let _ = fs::write(PREVIOUS_DEFAULT, previous);

        modify(&mut self.0)?;

        self.0
            .overwrite_loader_conf()
            .map_err(Error::DefaultBootSet)
    }

    /// Defines the specified entry as the default boot entry
    pub fn set_default_boot_id(&mut self, id: &str) -> Result<(), Error> {
        self.set_default_boot(|conf| {
            conf.loader_conf.default = Some(id.into());
            Ok(())
        })
    }

    /// Defines the specified entry as the default boot entry
    pub fn set_default_boot_variant(&mut self, variant: &LoaderEntry) -> Result<(), Error> {
        self.set_default_boot(|conf| {
            let comparison: fn(filename: &str) -> bool = match variant {
                LoaderEntry::Current => |e| e.to_lowercase().ends_with("current"),
                LoaderEntry::Recovery => |e| e.to_lowercase().starts_with("recovery"),
            };

            let recovery_entry = conf
                .entries
                .iter()
                .find(|e| comparison(&e.id))
                .ok_or(Error::MissingRecoveryEntry)?;

            conf.loader_conf.default = Some(recovery_entry.id.clone());
            Ok(())
        })
    }
}

pub enum LoaderEntry {
    Current,
    Recovery,
}

/// Restores the previous default boot entry
pub fn restore_default() -> Result<(), Error> {
    if !Path::new(PREVIOUS_DEFAULT).exists() {
        return Ok(());
    }

    let mut conf = BootConf::load()?;

    fs::read_to_string(PREVIOUS_DEFAULT)
        .map_err(Error::PreviousBootRead)
        .and_then(|entry| conf.set_default_boot_id(&entry))
        .and_then(|_| remove_previous())
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
        "/usr/lib/systemd/system/pop-upgrade-init.service",
        "/usr/lib/systemd/system/system-update.target.wants/pop-upgrade-init.service",
    ];

    let invalid = REQUIRED_UPGRADE_FILES
        .iter()
        .copied()
        .filter(|file| !Path::new(file).is_file())
        .collect::<Vec<&'static str>>();

    if !invalid.is_empty() {
        return Err(ReleaseError::SystemdUpgradeFilesMissing(invalid));
    }

    Ok(())
}

fn remove_previous() -> Result<(), Error> {
    fs::remove_file(PREVIOUS_DEFAULT).map_err(Error::RemovePreviousDefault)
}
