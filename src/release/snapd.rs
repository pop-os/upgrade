use super::errors::ReleaseError;
use apt_cmd::{AptCache, AptMark};
use async_fs as fs;

/// Holds all packages which have a pre-depend on snapd.
///
/// This should be executed after the source lists are upgraded to the new release,
/// and before packages have been fetched.
pub async fn hold_transitional_packages() -> Result<(), ReleaseError> {
    let mut out = String::new();
    let snap_packages = AptCache::predepends_of(&mut out, "snapd")
        .await
        .map_err(ReleaseError::TransitionalSnapFetch)?;

    let mut buffer = String::new();

    for package in &snap_packages {
        buffer.push_str(&*package);
        buffer.push('\n');
    }

    fs::write(crate::TRANSITIONAL_SNAPS, buffer.as_bytes())
        .await
        .map_err(ReleaseError::TransitionalSnapRecord)?;

    let _ = AptMark::new().hold(&snap_packages).await;

    Ok(())
}
