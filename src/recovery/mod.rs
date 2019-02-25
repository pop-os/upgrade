mod errors;

use atomic::Atomic;
use parallel_getter::ParallelGetter;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use sys_mount::{Mount, MountFlags, Unmount, UnmountFlags};
use tempfile::{tempdir, TempDir};

use crate::checksum::validate_checksum;
use crate::external::{findmnt_uuid, rsync};
use crate::release_api::Release;
use crate::release_architecture::detect_arch;
use crate::system_environment::SystemEnvironment;
use ubuntu_version::Version;

pub use self::errors::{RecResult, RecoveryError};

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
    Failed = 5,
}

impl From<RecoveryEvent> for &'static str {
    fn from(event: RecoveryEvent) -> Self {
        match event {
            RecoveryEvent::Fetching => "fetching recovery files",
            RecoveryEvent::Syncing => "syncing recovery files with recovery partition",
            RecoveryEvent::Verifying => "verifying checksums of fetched files",
            RecoveryEvent::Complete => "recovery partition upgrade completed",
            RecoveryEvent::Failed => "recovery partition upgrade failed",
        }
    }
}

#[derive(Debug, Clone)]
pub enum UpgradeMethod {
    FromFile(PathBuf),
    FromRelease { version: Option<String>, arch: Option<String>, flags: ReleaseFlags },
}

pub fn recovery<F, E>(action: &UpgradeMethod, progress: F, event: E) -> RecResult<()>
where
    F: Fn(u64, u64) + 'static + Send + Sync,
    E: Fn(RecoveryEvent) + 'static,
{
    if SystemEnvironment::detect() != SystemEnvironment::Efi {
        return Err(RecoveryError::Unsupported);
    }

    // Check the system and perform any repairs necessary for success.
    crate::repair::repair().map_err(RecoveryError::Repair)?;

    if !Path::new("/recovery").is_dir() {
        return Err(RecoveryError::RecoveryNotFound);
    }

    // The function must be Arc'd so that it can be borrowed.
    // Borrowck disallows moving ownership due to using FnMut instead of FnOnce.
    let progress = Arc::new(progress);

    let result = fetch_iso(&action, &progress, &event, Path::new("/recovery"));
    if result.is_err() {
        event(RecoveryEvent::Failed);
    }

    result.map(|_| ())
}

fn fetch_iso<F: Fn(u64, u64) + 'static + Send + Sync>(
    action: &UpgradeMethod,
    progress: &Arc<F>,
    event: &dyn Fn(RecoveryEvent),
    recovery_path: &Path,
) -> RecResult<()> {
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
    let iso = match action {
        UpgradeMethod::FromRelease { ref version, ref arch, flags } => {
            from_release(&mut temp_iso_dir, progress, event, version, arch, *flags)?
        }
        UpgradeMethod::FromFile(ref path) => from_file(path)?,
    };

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

    Ok(())
}

/// Fetches the release ISO remotely from api.pop-os.org.
fn from_release<F: Fn(u64, u64) + 'static + Send + Sync>(
    temp: &mut Option<TempDir>,
    progress: &Arc<F>,
    event: &dyn Fn(RecoveryEvent),
    version: &Option<String>,
    arch: &Option<String>,
    flags: ReleaseFlags,
) -> RecResult<PathBuf> {
    let version_: String;
    let version = match version {
        Some(ref version) => version,
        None => {
            let current = Version::detect()?;
            let version =
                if flags.contains(ReleaseFlags::NEXT) { current.next_release() } else { current };

            version_ = format!("{}", version);
            &version_
        }
    };

    let arch = match arch {
        Some(ref arch) => arch,
        None => detect_arch()?,
    };

    let release = Release::get_release(version, arch).map_err(RecoveryError::ApiError)?;
    from_remote(temp, progress, event, &release.url, &release.sha_sum)
        .map_err(|why| RecoveryError::Download(Box::new(why)))
}

/// Check that the file exist.
fn from_file(path: &PathBuf) -> RecResult<PathBuf> {
    if path.exists() {
        Ok(path.clone())
    } else {
        Err(RecoveryError::IsoNotFound)
    }
}

/// Downloads the ISO from a remote location, to a temporary local directory.
///
/// Once downloaded, the ISO will be verfied against the given checksum.
fn from_remote<F: Fn(u64, u64) + 'static + Send + Sync>(
    temp_dir: &mut Option<TempDir>,
    progress: &Arc<F>,
    event: &dyn Fn(RecoveryEvent),
    url: &str,
    checksum: &str,
) -> RecResult<PathBuf> {
    info!("downloading ISO from remote at {}", url);
    let temp = tempdir().map_err(RecoveryError::TempDir)?;
    let path = temp.path().join("new.iso");

    let mut file =
        OpenOptions::new().create(true).write(true).read(true).truncate(true).open(&path)?;

    let progress_ = progress.clone();
    let total = Arc::new(Atomic::new(0));
    let total_ = total.clone();
    ParallelGetter::new(url, &mut file)
        .threads(8)
        .callback(
            1000,
            Box::new(move |p, t| {
                total_.store(t / 1024, Ordering::SeqCst);
                (*progress_)(p / 1024, t / 1024);
            }),
        )
        .get()
        .map_err(|why| RecoveryError::Fetch { url: url.to_owned(), why })?;

    let total = total.load(Ordering::SeqCst);
    (*progress)(total, total);
    (*event)(RecoveryEvent::Verifying);

    file.flush()?;
    file.seek(SeekFrom::Start(0))?;

    validate_checksum(&mut file, checksum)
        .map_err(|why| RecoveryError::Checksum { path: path.clone(), why })?;

    *temp_dir = Some(temp);
    Ok(path)
}
