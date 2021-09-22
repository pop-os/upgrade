use anyhow::Context;
use std::{fs, path::Path};

pub fn disable() -> anyhow::Result<()> {
    info!("attempting to disable gnome-shell extensions");

    let (uid_min, uid_max) = crate::misc::uid_min_max()?;

    for user in unsafe { users::all_users() } {
        if user.uid() >= uid_min && user.uid() <= uid_max {
            let name = user.name();
            if let Some(name) = name.to_str() {
                info!("disabling gnome-shell extensions for {}", name);
                disable_for(name);
            }
        }
    }

    Ok(())
}

fn extension_path(user: &str) -> String {
    ["/home/", user, "/.local/share/gnome-shell/extensions"].concat()
}

fn disable_for(user: &str) {
    let path = extension_path(user);
    let backup = [&path, ".bak"].concat();

    let result = (|| {
        if Path::new(&backup).exists() {
            fs::remove_dir_all(&backup).context("cannot remove extensions backup")?;
        }

        fs::rename(&path, &backup).context("cannot backup extensions")
    })();

    if let Err(why) = result {
        error!("failed to disable gnome-shell extensions for {}: {}", user, why);
    };
}
