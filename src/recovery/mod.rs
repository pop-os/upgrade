pub mod check;

mod errors;
mod version;

use async_std::{
    fs as afs,
    path::{Path as APath, PathBuf as APathBuf},
    prelude::*,
};
use atomic::Atomic;
use parallel_getter::ParallelGetter;
use std::{
    fs::{self, OpenOptions},
    io::{self, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
};
use sys_mount::{Mount, MountFlags, Unmount, UnmountFlags};

use crate::{
    api::Release, checksum::validate_checksum, external::findmnt_uuid,
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
    cancel: &Arc<dyn Fn() -> bool + Send + Sync>,
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

    // The function must be Arc'd so that it can be borrowed.
    // Borrowck disallows moving ownership due to using FnMut instead of FnOnce.
    let progress = Arc::new(progress);

    fetch_iso(cancel, &action, &progress, &event, "/recovery")
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
    cancel: &Arc<dyn Fn() -> bool + Send + Sync>,
    action: &UpgradeMethod,
    progress: &Arc<F>,
    event: &dyn Fn(RecoveryEvent),
    recovery_path: P,
) -> RecResult<()> {
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

    futures::executor::block_on(async move {
        let recovery_uuid = findmnt_uuid(recovery_path)?;

        let iso = match action {
            UpgradeMethod::FromRelease { ref version, ref arch, flags } => {
                let version_ = version.as_ref().map(String::as_str);
                let arch_ = arch.as_ref().map(String::as_str);

                let (version, arch, release) = crate::release::check::current(version_, arch_)
                    .await
                    .ok_or(RecoveryError::NoBuildAvailable)?;

                let current = release.build.ok_or(RecoveryError::NoBuildAvailable)?;

                cancellation_check(&cancel)?;

                let cache =
                    Path::new(crate::CACHE_DIR).join([&*version, "-", arch, ".iso"].concat());

                if cache.exists()
                    && validate_checksum(&mut fs::File::open(&cache)?, &*current.sha).is_ok()
                {
                    info!("recovery partition has already been fetched");
                } else {
                    from_release(cancel, &*cache, progress, event, &*version, arch, *flags).await?;
                }

                cache
            }
            UpgradeMethod::FromFile(ref _path) => {
                // from_file(path)?
                unimplemented!();
            }
        };

        cancellation_check(&cancel)?;

        (*event)(RecoveryEvent::Syncing);

        let tempdir = tempfile::tempdir().map_err(RecoveryError::TempDir)?;
        let temppath = tempdir.path();

        info!("mounting iso");
        let _iso_mount = Mount::new(iso, temppath, "iso9660", MountFlags::RDONLY, None)?
            .into_unmount_drop(UnmountFlags::DETACH);

        let iso_casper = temppath.join("casper/");
        let iso_disk = temppath.join(".disk");
        let iso_dists = temppath.join("dists");
        let iso_pool = temppath.join("pool/");

        let rec_casper = ["/recovery/casper-", &*recovery_uuid].concat();
        let rec_disk = Path::new("/recovery/.disk");
        let rec_dists = Path::new("/recovery/dists");
        let rec_pool = Path::new("/recovery/pool");

        info!("removing prior recovery files");
        remove_prior_recovery().await?;

        let casper = sync(&iso_casper, Path::new(&rec_casper));
        let disk = sync(&iso_disk, rec_disk);
        let dists = sync(&iso_dists, rec_dists);
        let pool = sync(&iso_pool, rec_pool);

        info!("copying files");
        futures::try_join!(casper, disk, dists, pool)?;

        (*event)(RecoveryEvent::Complete);

        Ok(())
    })
}

/// Fetches the release ISO remotely from api.pop-os.org.
async fn from_release<F: Fn(u64, u64) + 'static + Send + Sync>(
    cancel: &Arc<dyn Fn() -> bool + Send + Sync>,
    path: &Path,
    progress: &Arc<F>,
    event: &dyn Fn(RecoveryEvent),
    version: &str,
    arch: &str,
    _flags: ReleaseFlags,
) -> RecResult<()> {
    let build = Release::fetch(version, arch)
        .await
        .map_err(RecoveryError::ApiError)?
        .build
        .ok_or(RecoveryError::NoBuildAvailable)?;

    from_remote(cancel, path, progress, event, &build.url, &build.sha)
        .map_err(|why| RecoveryError::Download(Box::new(why)))
}

/// Downloads the ISO from a remote location, to a temporary local directory.
///
/// Once downloaded, the ISO will be verfied against the given checksum.
fn from_remote<F: Fn(u64, u64) + 'static + Send + Sync>(
    cancel: &Arc<dyn Fn() -> bool + Send + Sync>,
    path: &Path,
    progress: &Arc<F>,
    event: &dyn Fn(RecoveryEvent),
    url: &str,
    checksum: &str,
) -> RecResult<()> {
    info!("downloading ISO from remote at {}", url);

    let mut file =
        OpenOptions::new().create(true).write(true).read(true).truncate(true).open(&path)?;

    let total = Arc::new(Atomic::new(0));
    ParallelGetter::new(url, &mut file)
        .threads(8)
        .callback(
            1000,
            Box::new(enclose!((total, progress, cancel) move |p, t| {
                total.store(t / 1024, Ordering::SeqCst);
                (*progress)(p / 1024, t / 1024);
                cancel()
            })),
        )
        .get()
        .map_err(|why| RecoveryError::Fetch { url: url.to_owned(), why })?;

    cancellation_check(cancel)?;

    let total = total.load(Ordering::SeqCst);
    (*progress)(total, total);
    (*event)(RecoveryEvent::Verifying);

    file.flush()?;
    file.seek(SeekFrom::Start(0))?;

    validate_checksum(&mut file, checksum)
        .map_err(|why| RecoveryError::Checksum { path: path.into(), why })?;

    cancellation_check(cancel)
}

fn cancellation_check(cancel: &Arc<dyn Fn() -> bool + Send + Sync>) -> RecResult<()> {
    if cancel() {
        Err(RecoveryError::Cancelled)
    } else {
        Ok(())
    }
}

async fn remove_prior_recovery() -> io::Result<()> {
    let mut stream = afs::read_dir("/recovery").await?;

    while let Some(entry) = stream.next().await {
        if let Ok(entry) = entry {
            let path = entry.path();
            if path.is_dir().await {
                info!("removing directory: {:?}", path);
                afs::remove_dir_all(path).await?;
            } else if let Some(name) = path.file_name() {
                if "recovery.conf"
                    != name.to_str().expect("corrupted filename in recovery partition")
                {
                    info!("removing file: {:?}", path);
                    afs::remove_file(path).await?;
                }
            }
        }
    }

    Ok(())
}

use walkdir::WalkDir;

async fn sync(source: &Path, dest: &Path) -> io::Result<()> {
    let mut links = Vec::new();

    walk_and_copy(&mut links, source, dest).await?;

    while let Some((link, dpath)) = links.pop() {
        walk_and_copy(&mut links, link.as_ref(), &dpath).await?;
    }

    Ok(())
}

async fn walk_and_copy(
    links: &mut Vec<(APathBuf, PathBuf)>,
    source: &Path,
    dest: &Path,
) -> io::Result<()> {
    for entry in WalkDir::new(source) {
        if let Ok(entry) = entry {
            let spath = entry.path();
            let dpath = dest.join(spath.strip_prefix(source).unwrap());

            if let Ok(metadata) = afs::symlink_metadata(spath).await {
                let ftype = metadata.file_type();

                if ftype.is_symlink() {
                    if let Ok(mut link) = afs::read_link(spath).await {
                        if link.is_relative() {
                            link = APath::new(&dest).join(&link);
                        }

                        links.push((link, dpath));
                    }
                } else if ftype.is_dir() {
                    info!("creating directory: {:?}", dpath);
                    afs::create_dir(dpath).await?;
                } else if ftype.is_file() {
                    info!("creating file: {:?}", dpath);
                    // NOTE: hard links unsupported for FAT32
                    afs::copy(spath, dpath).await?;
                }
            }
        }
    }

    Ok(())
}
