use as_result::MapResult;
use std::process::Command;

pub fn disable() -> anyhow::Result<()> {
    let (uid_min, uid_max) = crate::misc::uid_min_max()?;

    for user in unsafe { users::all_users() } {
        if user.uid() > uid_min && user.uid() < uid_max {
            let name = user.name();

            info!("disabling gnome-shell extensions for {}", name.to_str().unwrap_or("<unkown>"));

            disable_for(name);
        }
    }

    Ok(())
}

fn disable_for(user: &std::ffi::OsStr) {
    let result = Command::new("sudo")
        .arg("-Hu")
        .arg(user)
        .args(&["gsettings", "set", "org.gnome.shell", "disable-user-extensions", "true"])
        .status()
        .map_result();

    if let Err(why) = result {
        error!(
            "failed to disable gnome-shell extensions for {}: {}",
            user.to_str().unwrap_or("<unknown>"),
            why
        );
    };
}
