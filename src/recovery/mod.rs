mod errors;
mod fetch;
mod version;

use async_fetcher_preview::{
    checksum::{Checksum, SumStr},
    validate_checksum,
};
use futures03::executor;
use std::{
    convert::TryFrom,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use sys_mount::{Mount, MountFlags, Unmount, UnmountFlags};
use tempfile::{tempdir, TempDir};

use crate::{
    external::{findmnt_uuid, rsync},
    release_api::Release,
    release_architecture::detect_arch,
    system_environment::SystemEnvironment,
};

pub use self::{
    errors::{RecResult, RecoveryError},
    version::{recovery_file, version, RecoveryVersion, RecoveryVersionError, RECOVERY_VERSION},
};

bitflags! {
    pub struct ReleaseFlags: u8 {
        const NEXT = 1;
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, FromPrimitive, PartialEq)]
pub enum RecoveryEvent {
    Fetching = 1,
    Verifying = 2,
    Syncing = 3,
    Complete = 4,
}

impl From<RecoveryEvent> for &'static str {
    fn from(event: RecoveryEvent) -> Self {
        match event {
            RecoveryEvent::Fetching => "fetching recovery files",
            RecoveryEvent::Syncing => "syncing recovery files with recovery partition",
            RecoveryEvent::Verifying => "verifying checksums of fetched files",
            RecoveryEvent::Complete => "recovery partition upgrade completed",
        }
    }
}

#[derive(Debug, Clone)]
pub enum UpgradeMethod {
    FromFile(PathBuf),
    FromRelease { version: Option<String>, arch: Option<String>, flags: ReleaseFlags },
}

pub fn recovery<F, E>(
    cancel: &Arc<AtomicBool>,
    action: &UpgradeMethod,
    progress: F,
    event: E,
) -> RecResult<()>
where
    F: Fn(u64, u64) + 'static + Send + Sync,
    E: Fn(RecoveryEvent) + 'static,
{
    if SystemEnvironment::detect() != SystemEnvironment::Efi {
        return Err(RecoveryError::Unsupported);
    }

    // Check the system and perform any repairs necessary for success.
    crate::repair::repair().map_err(RecoveryError::Repair)?;

    cancellation_check(&cancel)?;

    if !recovery_exists()? {
        return Err(RecoveryError::RecoveryNotFound);
    }

    fn verify(version: &str, build: u16) -> bool {
        recovery_file()
            .ok()
            .and_then(move |string| {
                let mut iter = string.split_whitespace();
                let current_version = iter.next()?;
                let current_build = iter.next()?.parse::<u16>().ok()?;

                Some(version == current_version && build == current_build)
            })
            .unwrap_or(false)
    }

    // The function must be Arc'd so that it can be borrowed.
    // Borrowck disallows moving ownership due to using FnMut instead of FnOnce.
    let progress = Arc::new(progress);

    if let Some((version, build)) =
        fetch_iso(cancel, verify, &action, &progress, &event, "/recovery")?
    {
        let data = format!("{} {}", version, build);
        fs::write(RECOVERY_VERSION, data.as_bytes()).map_err(RecoveryError::WriteVersion)?;
    }

    Ok(())
}

pub fn recovery_exists() -> Result<bool, RecoveryError> {
    let mounts = proc_mounts::MountIter::new().map_err(RecoveryError::Mounts)?;

    for mount in mounts {
        let mount = mount.map_err(RecoveryError::Mounts)?;
        if mount.dest == Path::new("/recovery") {
            return Ok(true);
        }
    }

    Ok(false)
}

fn fetch_iso<P: AsRef<Path>, F: Fn(u64, u64) + 'static + Send + Sync>(
    cancel: &Arc<AtomicBool>,
    verify: fn(&str, u16) -> bool,
    action: &UpgradeMethod,
    progress: &Arc<F>,
    event: &dyn Fn(RecoveryEvent),
    recovery_path: P,
) -> RecResult<Option<(String, u16)>> {
    let recovery_path = recovery_path.as_ref();
    info!("fetching ISO to upgrade recovery partition at {}", recovery_path.display());
    (*event)(RecoveryEvent::Fetching);

    if !recovery_path.exists() {
        return Err(RecoveryError::RecoveryNotFound);
    }

    let efi_path = Path::new("/boot/efi/EFI/");
    if !efi_path.exists() {
        return Err(RecoveryError::EfiNotFound);
    }

    let recovery_uuid = findmnt_uuid(recovery_path)?;
    let casper = ["casper-", &recovery_uuid].concat();
    let recovery = ["Recovery-", &recovery_uuid].concat();

    let mut temp_iso_dir = None;
    let (build, version, iso) = match action {
        UpgradeMethod::FromRelease { ref version, ref arch, flags } => {
            let version_ = version.as_ref().map(String::as_str);
            let arch = arch.as_ref().map(String::as_str);

            let (version, build) =
                crate::release::check_current(version_).ok_or(RecoveryError::NoBuildAvailable)?;

            cancellation_check(&cancel)?;

            if verify(&version, build) {
                info!("recovery partition is already upgraded to {}b{}", version, build);
                return Ok(None);
            }

            cancellation_check(&cancel)?;

            let iso =
                from_release(cancel, &mut temp_iso_dir, progress, event, &version, arch, *flags)?;
            (build, version, iso)
        }
        UpgradeMethod::FromFile(ref _path) => {
            // from_file(path)?
            unimplemented!();
        }
    };

    cancellation_check(&cancel)?;

    (*event)(RecoveryEvent::Syncing);
    let tempdir = tempfile::tempdir().map_err(RecoveryError::TempDir)?;
    let _iso_mount = Mount::new(iso, tempdir.path(), "iso9660", MountFlags::RDONLY, None)?
        .into_unmount_drop(UnmountFlags::DETACH);

    let disk = tempdir.path().join(".disk");
    let dists = tempdir.path().join("dists");
    let pool = tempdir.path().join("pool");
    let casper_p = tempdir.path().join("casper/");
    let efi_recovery = efi_path.join(&recovery);
    let efi_initrd = efi_recovery.join("initrd.gz");
    let efi_vmlinuz = efi_recovery.join("vmlinuz.efi");
    let casper_initrd = recovery_path.join([&casper, "/initrd.gz"].concat());
    let casper_vmlinuz = recovery_path.join([&casper, "/vmlinuz.efi"].concat());
    let recovery_str = recovery_path.to_str().unwrap();

    rsync(&[&disk, &dists, &pool], recovery_str, &["-KLavc", "--inplace", "--delete"])?;

    rsync(
        &[&casper_p],
        &[recovery_str, "/", &casper].concat(),
        &["-KLavc", "--inplace", "--delete"],
    )?;

    crate::misc::cp(&casper_initrd, &efi_initrd)?;
    crate::misc::cp(&casper_vmlinuz, &efi_vmlinuz)?;

    (*event)(RecoveryEvent::Complete);

    Ok(Some((version, build)))
}

/// Fetches the release ISO remotely from api.pop-os.org.
fn from_release<F: Fn(u64, u64) + 'static + Send + Sync>(
    cancel: &Arc<AtomicBool>,
    temp: &mut Option<TempDir>,
    progress: &Arc<F>,
    event: &dyn Fn(RecoveryEvent),
    version: &str,
    arch: Option<&str>,
    _flags: ReleaseFlags,
) -> RecResult<PathBuf> {
    let arch = match arch {
        Some(ref arch) => arch,
        None => detect_arch()?,
    };

    let release = Release::get_release(version, arch).map_err(RecoveryError::ApiError)?;

    let checksum = Checksum::try_from(SumStr::Sha256(&release.sha_sum))?;

    let remote_fetch = from_remote(cancel, temp, progress, &release.url, release.size);

    let iso_path = executor::block_on(async move {
        let path = remote_fetch.await.map_err(|why| RecoveryError::Download(Box::new(why)))?;

        (event)(RecoveryEvent::Verifying);
        validate_checksum(&mut [0u8; 8 * 1024], &path, &checksum).await?;
        Ok::<_, RecoveryError>(path)
    })?;

    Ok(iso_path)
}

/// Downloads the ISO from a remote location, to a temporary local directory.
///
/// Once downloaded, the ISO will be verfied against the given checksum.
async fn from_remote<F: Fn(u64, u64) + 'static + Send + Sync>(
    cancel: &Arc<AtomicBool>,
    temp_dir: &mut Option<TempDir>,
    progress: &Arc<F>,
    url: &str,
    iso_size: u64,
) -> RecResult<PathBuf> {
    info!("downloading ISO from remote at {}", url);
    let temp = tempdir().map_err(RecoveryError::TempDir)?;
    let path = temp.path().join("new.iso");

    let future = fetch::fetch(path.clone().into(), url.into(), iso_size, progress.clone(), cancel);

    future.await.map_err(|why| RecoveryError::Fetch { url: url.into(), why })?;

    *temp_dir = Some(temp);
    Ok(path)
}

fn cancellation_check(cancel: &Arc<AtomicBool>) -> RecResult<()> {
    if cancel.load(Ordering::SeqCst) {
        Err(RecoveryError::Cancelled)
    } else {
        Ok(())
    }
}
