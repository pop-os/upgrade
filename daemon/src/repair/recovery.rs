use crate::ubuntu_version::Codename;

error_set::error_set! {
    Error := {
        #[display("failed to load EFI boot loader configurations")]
        BootConfLoad(crate::release::systemd::Error),
        #[display("failed to update recovery loader entry")]
        UpdateRecoveryEntry(systemd_boot_conf::Error),
    }
}

pub fn repair(codename: Codename) -> Result<(), Error> {
    // Resolute requires updating kernel boot options for dracut
    if let Codename::Resolute = codename {
        return apply_dracut_boot_options();
    }

    Ok(())
}

/// Apply 26.04 dracut migrations for the recovery partition
pub fn apply_dracut_boot_options() -> Result<(), Error> {
    let mut boot_conf = crate::release::systemd::BootConf::load()
        .map_err(Error::BootConfLoad)?
        .0;

    let mut updates = Vec::new();
    for entry in boot_conf.entries.iter_mut().filter(|entry| {
        entry.id.starts_with("Recovery-")
            && entry
                .options
                .iter()
                .any(|option| &**option == "boot=casper")
    }) {
        let mut new_options = Vec::new();
        for option in std::mem::take(&mut entry.options) {
            if &*option == "boot=casper" {
                new_options.push("rd.live.image".into());
                new_options.push("rd.live.squashimg=filesystem.squashfs".into());
                new_options.push("rd.live.overlay".into());
            } else if let Some(live_dir) = option.strip_prefix("live-media-path=") {
                new_options.push(["rd.live.dir=", live_dir].concat().into_boxed_str());
            } else if let Some(partuuid) = option.strip_prefix("live-media=/dev/disk/by-partuuid/")
            {
                new_options.push(["root=live:PARTUUID=", partuuid].concat().into_boxed_str());
            } else {
                new_options.push(option);
            }
        }

        entry.options = new_options;
        updates.push(entry.id.clone());
    }

    for id in updates {
        boot_conf
            .overwrite_entry_conf(&*id)
            .map_err(Error::UpdateRecoveryEntry)?;
    }

    Ok(())
}
