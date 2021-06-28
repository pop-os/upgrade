use as_result::MapResult;
use std::{
    process::Command,
    str,
};

const SETTINGS_JS_SCRIPT: &str = "
const Gio = imports.gi.Gio;

const COSMIC_FAVORITES = [
    'pop-cosmic-launcher.desktop',
    'pop-cosmic-workspaces.desktop',
    'pop-cosmic-applications.desktop',
];

const settings = new Gio.Settings({schema_id: 'org.gnome.shell'});
const favorites = settings.get_strv('favorite-apps');
settings.set_strv('favorite-apps', COSMIC_FAVORITES.concat(favorites));
";

pub fn update_favorites() -> anyhow::Result<()> {
    let (uid_min, uid_max) = crate::misc::uid_min_max()?;

    for user in unsafe { users::all_users() } {
        if user.uid() >= uid_min && user.uid() <= uid_max {
            let name = user.name();

            info!("updating favorite-apps for {}", name.to_str().unwrap_or("<unkown>"));

            if let Err(why) = update_favorites_for(name) {
                error!(
                     "failed to update favorite-apps for {}: {}",
                     name.to_str().unwrap_or("<unknown>"),
                     why
                )
            }
        }
    }

    Ok(())
}

fn update_favorites_for(user: &std::ffi::OsStr) -> anyhow::Result<()> {
    // If `dconf read` returns nothing, there is no entry in the user's dconf
    // database, so gsettings will just use the system default
    let is_default = Command::new("sudo")
        .arg("-Hu")
        .arg(user)
        .args(&["dconf", "read", "/org/gnome/shell/favorite-apps"])
        .output()?
        .stdout
        .is_empty();

    // Otherwise if user has set favorites, prepend Cosmic favorites
    if !is_default {
        Command::new("sudo")
            .arg("-Hu")
            .arg(user)
            .arg("dbus-launch")
            .args(&["gjs", "-c", SETTINGS_JS_SCRIPT])
            .status()
            .map_result()?;
    }

    Ok(())
}
