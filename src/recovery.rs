mod errors;
mod version;

use anyhow::anyhow;
use as_result::MapResult;
use async_process::Command;
use bitflags::bitflags;
use cascade::cascade;
use futures::prelude::*;
use num_derive::FromPrimitive;
use std::{
    io::SeekFrom,
    path::{Path, PathBuf},
    time::Instant,
};
use sys_mount::{Mount, MountFlags, Unmount, UnmountFlags};
use tempfile::{tempdir, TempDir};

use crate::{
    checksum::validate_checksum, external::findmnt_uuid, release_api::Release,
    release_architecture::detect_arch, system_environment::SystemEnvironment,
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

pub async fn recovery<'a, F, E>(
    cancel: &'a (dyn Fn() -> bool + Send + Sync),
    action: &'a UpgradeMethod,
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
    crate::repair::repair().await.map_err(RecoveryError::Repair)?;

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

    if let Some((version, build)) =
        fetch_iso(cancel, verify, &action, &progress, &event, "/recovery").await?
    {
        let data = format!("{} {}", version, build);
        async_fs::write(RECOVERY_VERSION, data.as_bytes())
            .await
            .map_err(RecoveryError::WriteVersion)?;
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

async fn fetch_iso<'a, P: AsRef<Path>, F: Fn(u64, u64) + 'static + Send + Sync>(
    cancel: &'a (dyn Fn() -> bool + Send + Sync),
    verify: fn(&str, u16) -> bool,
    action: &'a UpgradeMethod,
    progress: &'a F,
    event: &'a dyn Fn(RecoveryEvent),
    recovery_path: P,
) -> RecResult<Option<(Box<str>, u16)>> {
    let recovery_path = recovery_path.as_ref();
    log::info!("fetching ISO to upgrade recovery partition at {}", recovery_path.display());
    (*event)(RecoveryEvent::Fetching);

    if !recovery_path.exists() {
        return Err(RecoveryError::RecoveryNotFound);
    }

    let efi_path = Path::new("/boot/efi/EFI/");
    if !efi_path.exists() {
        return Err(RecoveryError::EfiNotFound);
    }

    let recovery_uuid = findmnt_uuid(recovery_path).await?;

    let casper = ["casper-", &recovery_uuid].concat();
    let recovery = ["Recovery-", &recovery_uuid].concat();

    // TODO: Create recovery entry if it is missing
    std::fs::create_dir_all(&recovery)?;

    let mut temp_iso_dir = None;
    let (build, version, iso) = match action {
        UpgradeMethod::FromRelease { ref version, ref arch, flags } => {
            let version_ = version.as_ref().map(String::as_str);
            let arch = arch.as_ref().map(String::as_str);

            let (version, build) =
                crate::release::check::current(version_).ok_or(RecoveryError::NoBuildAvailable)?;

            cancellation_check(&cancel)?;

            if verify(&version, build) {
                log::info!("recovery partition is already upgraded to {}b{}", version, build);
                return Ok(None);
            }

            cancellation_check(&cancel)?;

            let iso =
                from_release(cancel, &mut temp_iso_dir, progress, event, &version, arch, *flags)
                    .await?;
            (build, version, iso)
        }
        UpgradeMethod::FromFile(ref _path) => {
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

    let mut cmd = cascade! {
        Command::new("rsync");
        ..args(&[&disk, &dists, &pool]);
        ..arg(recovery_str);
        ..args(&["-KLavc", "--inplace", "--delete"]);
    };

    cmd.status().await.map_result()?;

    let mut cmd = cascade! {
        Command::new("rsync");
        ..args(&[&casper_p]);
        ..arg(&[recovery_str, "/", &casper].concat());
        ..args(&["-KLavc", "--inplace", "--delete"]);
    };

    cmd.status().await.map_result()?;

    let cp1 = crate::misc::cp(&casper_initrd, &efi_initrd);
    let cp2 = crate::misc::cp(&casper_vmlinuz, &efi_vmlinuz);

    futures::try_join!(cp1, cp2)?;

    (*event)(RecoveryEvent::Complete);

    Ok(Some((version, build)))
}

/// Fetches the release ISO remotely from api.pop-os.org.
async fn from_release<'a, F: Fn(u64, u64) + 'static + Send + Sync>(
    cancel: &'a (dyn Fn() -> bool + Send + Sync),
    temp: &'a mut Option<TempDir>,
    progress: &'a F,
    event: &'a dyn Fn(RecoveryEvent),
    version: &'a str,
    arch: Option<&'a str>,
    _flags: ReleaseFlags,
) -> RecResult<PathBuf> {
    let arch = match arch {
        Some(ref arch) => arch,
        None => detect_arch()?,
    };

    let release = Release::get_release(version, arch).map_err(RecoveryError::ApiError)?;
    let iso_path = from_remote(cancel, temp, progress, event, &release.url, &release.sha_sum)
        .await
        .map_err(|why| RecoveryError::Download(Box::new(why)))?;

    Ok(iso_path)
}

/// Downloads the ISO from a remote location, to a temporary local directory.
///
/// Once downloaded, the ISO will be verfied against the given checksum.
async fn from_remote<'a, F: Fn(u64, u64) + 'static + Send + Sync>(
    cancel: &'a (dyn Fn() -> bool + Send + Sync),
    temp_dir: &'a mut Option<TempDir>,
    progress: &'a F,
    event: &'a dyn Fn(RecoveryEvent),
    url: &'a str,
    checksum: &'a str,
) -> RecResult<PathBuf> {
    log::info!("downloading ISO from remote at {}", url);
    let temp = tempdir().map_err(RecoveryError::TempDir)?;
    let path = temp.path().join("new.iso");

    let mut file = async_fs::OpenOptions::new()
        .create(true)
        .write(true)
        .read(true)
        .truncate(true)
        .open(&path)
        .await?;

    let mut total = 0;

    (async {
        let req = isahc::get_async(url).await?;

        let status = req.status();
        if !status.is_success() {
            return Err(anyhow!("request failed due to status code {}", status));
        }

        total = req
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
            / 1024;

        let mut buf = vec![0u8; 8 * 1024];
        let mut p = 0;

        let mut body = req.into_body();

        let mut last = Instant::now();

        loop {
            let read = body.read(&mut buf).await?;
            if read == 0 {
                break;
            }

            file.write_all(&buf[..read]).await?;

            p += read;

            if last.elapsed().as_secs() > 1 {
                last = Instant::now();
                (*progress)(p as u64 / 1024, total);
            }

            cancellation_check(cancel)?;
        }

        Ok(())
    })
    .await
    .map_err(|source| RecoveryError::Fetch { url: url.to_owned(), source })?;

    cancellation_check(cancel)?;

    (*progress)(total, total);
    (*event)(RecoveryEvent::Verifying);

    file.flush().await?;
    file.seek(SeekFrom::Start(0)).await?;

    validate_checksum(&mut file, checksum)
        .await
        .map_err(|source| RecoveryError::Checksum { path: path.clone(), source })?;

    cancellation_check(cancel)?;

    *temp_dir = Some(temp);
    Ok(path)
}

fn cancellation_check(cancel: &(dyn Fn() -> bool + Send + Sync)) -> RecResult<()> {
    if cancel() {
        Err(RecoveryError::Cancelled)
    } else {
        Ok(())
    }
}
