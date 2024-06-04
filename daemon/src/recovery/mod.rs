mod errors;
mod version;

use crate::daemon::SignalEvent;
use anyhow::Context;
use async_fetcher::{Checksum, FetchEvent, Fetcher, SumStr};
use async_shutdown::ShutdownManager as Shutdown;
use std::{
    convert::TryFrom,
    path::{Path, PathBuf},
    sync::Arc,
};
use sys_mount::{Mount, MountFlags, Unmount, UnmountFlags};
use tokio::{process::Command, sync::mpsc::UnboundedSender};

use crate::{
    external::findmnt_uuid, release_api::Release, release_architecture::detect_arch,
    system_environment::SystemEnvironment,
};

pub use self::{
    errors::{RecResult, RecoveryError},
    version::{recovery_file, version, RecoveryVersion, RecoveryVersionError, RECOVERY_VERSION},
};

bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub struct ReleaseFlags: u8 {
        const NEXT = 1;
    }
}

const CACHE_PATH: &str = "/var/cache/pop-upgrade/";

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

pub async fn recovery(
    cancel: Shutdown<()>,
    action: &UpgradeMethod,
    sender: UnboundedSender<SignalEvent>,
) -> RecResult<()> {
    if SystemEnvironment::detect() != SystemEnvironment::Efi {
        return Err(RecoveryError::Unsupported);
    }

    // Check the system and perform any repairs necessary for success.
    crate::repair::repair().await.map_err(RecoveryError::Repair)?;

    shutdown_check(&cancel)?;

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

    if let Some((version, build)) = fetch_iso(cancel, verify, action, sender, "/recovery").await? {
        let data = fomat!((version) " " (build));
        tokio::fs::write(RECOVERY_VERSION, data.as_bytes())
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

async fn fetch_iso<P: AsRef<Path>>(
    cancel: Shutdown<()>,
    verify: fn(&str, u16) -> bool,
    action: &UpgradeMethod,
    sender: UnboundedSender<SignalEvent>,
    recovery_path: P,
) -> RecResult<Option<(Box<str>, u16)>> {
    let recovery_path = recovery_path.as_ref();
    info!("fetching ISO to upgrade recovery partition at {}", recovery_path.display());
    emit_recovery_event(&sender, RecoveryEvent::Fetching);

    if !recovery_path.exists() {
        return Err(RecoveryError::RecoveryNotFound);
    }

    let efi_path = Path::new("/boot/efi/EFI/");
    if !efi_path.exists() {
        return Err(RecoveryError::EfiNotFound);
    }

    let recovery_uuid =
        findmnt_uuid(recovery_path).await.context("cannot find UUID of recover partition")?;

    let casper = ["casper-", &recovery_uuid].concat();
    let recovery = ["Recovery-", &recovery_uuid].concat();
    let efi_recovery = efi_path.join(&recovery);

    // TODO: Create recovery entry if it is missing
    std::fs::create_dir_all(&efi_recovery).context("failed to create recovery entry directory")?;

    let (build, version, iso) = match action {
        UpgradeMethod::FromRelease { ref version, ref arch, .. } => {
            let version_ = version.as_ref().map(String::as_str);
            let arch = arch.as_ref().map(String::as_str);

            let (version, build) =
                crate::release::check::current(version_).await.context("no build available")?;

            shutdown_check(&cancel)?;

            if verify(&version, build) {
                info!("recovery partition is already upgraded to {}b{}", version, build);
                return Ok(None);
            }

            shutdown_check(&cancel)?;

            let cancel = cancel.clone();

            // Fetch the latest ISO from the release repository.
            let iso = (|| async {
                let arch = match arch {
                    Some(arch) => arch,
                    None => detect_arch()?,
                };

                shutdown_check(&cancel)?;

                let release =
                    Release::get_release(&version, arch).await.map_err(RecoveryError::ApiError)?;

                shutdown_check(&cancel)?;

                let iso_path = from_remote(
                    cancel.clone(),
                    sender.clone(),
                    release.url.into(),
                    &release.sha_sum,
                )
                .await?;

                shutdown_check(&cancel)?;

                Ok::<PathBuf, RecoveryError>(iso_path)
            })()
            .await?;

            (build, version, iso)
        }
        UpgradeMethod::FromFile(ref _path) => {
            unimplemented!();
        }
    };

    let _shutdown_delay = match cancel.delay_shutdown_token() {
        Ok(token) => token,
        Err(_) => return Err(RecoveryError::Cancelled),
    };

    emit_recovery_event(&sender, RecoveryEvent::Syncing);
    let tempdir = tempfile::tempdir().map_err(RecoveryError::TempDir)?;
    let _iso_mount = Mount::builder()
        .fstype("iso9660")
        .flags(MountFlags::RDONLY)
        .mount(iso, tempdir.path())
        .context("failed to mount recovery ISO")?
        .into_unmount_drop(UnmountFlags::DETACH);

    let disk = tempdir.path().join(".disk");
    let dists = tempdir.path().join("dists");
    let pool = tempdir.path().join("pool");
    let casper_p = tempdir.path().join("casper/");

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

    cmd.status().await.context("rsync failed to copy")?;

    let mut cmd = cascade! {
        Command::new("rsync");
        ..args(&[&casper_p]);
        ..arg(&[recovery_str, "/", &casper].concat());
        ..args(&["-KLavc", "--inplace", "--delete"]);
    };

    cmd.status().await.context("rsync failed to copy casper")?;

    let cp1 = crate::misc::cp(&casper_initrd, &efi_initrd);
    let cp2 = crate::misc::cp(&casper_vmlinuz, &efi_vmlinuz);

    futures::future::try_join(cp1, cp2).await.context("failed to copy kernel to recovery")?;

    emit_recovery_event(&sender, RecoveryEvent::Complete);

    Ok(Some((version, build)))
}

/// Downloads the ISO from a remote location, to a temporary local directory.
///
/// Once downloaded, the ISO will be verfied against the given checksum.
async fn from_remote(
    cancel: Shutdown<()>,
    sender: UnboundedSender<SignalEvent>,
    url: Box<str>,
    checksum_str: &str,
) -> RecResult<PathBuf> {
    let _ = std::fs::create_dir_all(CACHE_PATH);

    let path = Path::new(CACHE_PATH).join("recovery.iso");

    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }

    info!("downloading ISO from remote at {} to {:?}", url, path);

    let checksum = Checksum::try_from(SumStr::Sha256(checksum_str)).map_err(|source| {
        RecoveryError::ChecksumInvalid { checksum: checksum_str.to_owned(), source }
    })?;

    let (events_tx, mut events_rx) = tokio::sync::mpsc::unbounded_channel();

    let mut total = 0;

    let sender_ = sender.clone();
    let path_ = path.clone();
    let result = tokio::spawn(async move {
        info!("Initiating fetch of recovery ISO");

        let urls = Arc::from(vec![url.clone()]);
        let dest = Arc::from(path_.clone());

        rustix::fs::sync();

        Fetcher::default()
            // Timeout if a read takes more than 5 seconds.
            .timeout(std::time::Duration::from_secs(5))
            // Download at most 4 parts at a time.
            .connections_per_file(4)
            // 4 MiB partial files.
            .max_part_size(4 * 1024 * 1024)
            // Forward progress events to this sender.
            .events(events_tx)
            // Use this to watch for shutdown events.
            .shutdown(cancel)
            // Wrap in Arc
            .build()
            // Fetch the ISO to `dest`
            .request(urls, dest, Arc::new(()))
            .await
            .map_err(|source| RecoveryError::Fetch { url: url.into(), source })?;

        info!("fetched recovery ISO. Now validating checksum.");

        let sender = sender_.clone();
        let join_handle = tokio::task::spawn_blocking(move || {
            let file = std::fs::File::open(&path_).map_err(|_| RecoveryError::IsoNotFound)?;
            let _ = sender.send(SignalEvent::RecoveryUpgradeEvent(RecoveryEvent::Verifying));
            let result = checksum
                .validate(file, &mut vec![0u8; 16 * 1024])
                .map_err(|source| RecoveryError::Checksum { path: path_.clone(), source });

            if result.is_err() {
                let _ = std::fs::remove_file(&path_);
            }

            result
        });

        join_handle.await?
    });

    let mut progress = 0;
    let mut last_update = std::time::Instant::now();

    // Watch for events received by the fetcher as it is in progress.
    while let Some((_, _, event)) = events_rx.recv().await {
        match event {
            FetchEvent::ContentLength(t) => {
                total = t / 1024;
            }

            FetchEvent::Progress(p) => {
                progress += p;
                let now = std::time::Instant::now();
                if now.duration_since(last_update).as_secs() >= 1 {
                    emit_progress(&sender, progress / 1024, total);
                    last_update = now;
                }
            }

            FetchEvent::Retrying => {
                progress = 0;
            }

            _ => (),
        }
    }

    info!("recovery ISO fetch complete");

    result.await??;

    Ok(path)
}

fn emit_progress(sender: &UnboundedSender<SignalEvent>, progress: u64, total: u64) {
    let _ = sender.send(SignalEvent::RecoveryDownloadProgress(progress, total));
}

fn emit_recovery_event(sender: &UnboundedSender<SignalEvent>, event: RecoveryEvent) {
    let _ = sender.send(SignalEvent::RecoveryUpgradeEvent(event));
}

fn shutdown_check(shutdown: &Shutdown<()>) -> Result<(), RecoveryError> {
    if shutdown.is_shutdown_triggered() || shutdown.is_shutdown_completed() {
        Err(RecoveryError::Cancelled)
    } else {
        Ok(())
    }
}
