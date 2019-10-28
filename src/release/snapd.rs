use super::{apt_hold, errors::ReleaseError};
use apt_cli_wrappers::predepends_of;
use std::fs;

/// Holds all packages which have a pre-depend on snapd.
///
/// This should be executed after the source lists are upgraded to the new release,
/// and before packages have been fetched.
pub fn hold_transitional_packages() -> Result<(), ReleaseError> {
    let mut buffer = String::new();
    let snap_packages = predepends_of(&mut buffer, "snapd")
        .map_err(ReleaseError::TransitionalSnapFetch)?
        .collect::<Vec<&str>>();

    let mut buffer = String::new();

    for package in &snap_packages {
        buffer.push_str(*package);
        buffer.push('\n');
    }

    fs::write(crate::TRANSITIONAL_SNAPS, buffer.as_bytes())
        .map_err(ReleaseError::TransitionalSnapRecord)?;

    for package in &snap_packages {
        apt_hold(*package).map_err(ReleaseError::TransitionalSnapHold)?;
    }

    Ok(())
}
