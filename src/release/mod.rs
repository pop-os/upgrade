pub mod check;
pub mod eol;
pub mod repos;
pub mod systemd;

mod errors;
mod recovery;
mod snapd;

use self::systemd::LoaderEntry;

pub use self::{
    check::{BuildStatus, ReleaseStatus},
    errors::{RelResult, ReleaseError},
};
use crate::repair::{self, RepairError};

use anyhow::Context;
use apt_cmd::{
    lock::apt_lock_wait, request::Request as AptRequest, AptGet, AptMark, AptUpgradeEvent, Dpkg,
    DpkgQuery,
};
use futures_util::StreamExt;

use std::{
    collections::HashSet,
    convert::TryFrom,
    fs::{self, File},
    os::unix::fs::symlink,
    path::Path,
    sync::Arc,
};
use systemd_boot_conf::SystemdBootConf;

use ubuntu_version::{Codename, Version};

pub const STARTUP_UPGRADE_FILE: &str = "/pop-upgrade";

/// Packages which should be removed before upgrading.
///
/// - `gnome-software` conflicts with `pop-desktop` and its `sessioninstaller` dependency
/// - `ureadahead` was deprecated and removed from the repositories
const REMOVE_PACKAGES: &[&str] = &["gnome-software", "ureadahead", "backport-iwlwifi-dkms"];

/// Packages which should be installed before upgrading.
///
/// - `linux-generic` because some systems may have a different kernel installed
/// - `pop-desktop` because it pulls in all of our required desktop dependencies
/// - `sessioninstaller` because it may have been removed by `gnome-software`
const CORE_PACKAGES: &[&str] = &["linux-generic", "pop-desktop", "sessioninstaller"];

const DPKG_LOCK: &str = "/var/lib/dpkg/lock";
const LISTS_LOCK: &str = "/var/lib/apt/lists/lock";
const RELEASE_FETCH_FILE: &str = "/pop_preparing_release_upgrade";
const SYSTEM_UPDATE: &str = "/system-update";
const SYSTEMD_BOOT_LOADER_PATH: &str = "/boot/efi/loader";
const SYSTEMD_BOOT_LOADER: &str = "/boot/efi/EFI/systemd/systemd-bootx64.efi";

pub fn upgrade_in_progress() -> bool {
    Path::new(STARTUP_UPGRADE_FILE).exists() || Path::new(RELEASE_FETCH_FILE).exists()
}

#[repr(u8)]
#[derive(Copy, Clone, Debug)]
pub enum RefreshOp {
    Status = 0,
    Enable = 1,
    Disable = 2,
}

/// Configure the system to refresh the OS in the recovery partition.
pub fn refresh_os(op: RefreshOp) -> Result<bool, ReleaseError> {
    recovery::upgrade_prereq()?;

    let mut conf = systemd::BootConf::load().map_err(ReleaseError::RecoveryConf)?;

    match op {
        RefreshOp::Disable => {
            info!("Disabling refresh OS");

            conf.set_default_boot_variant(&LoaderEntry::Current)
                .map_err(ReleaseError::SystemdBoot)?;

            recovery::mode_unset().map_err(|why| ReleaseError::RecoveryConf(why.into()))?;

            Ok(false)
        }
        RefreshOp::Enable => {
            info!("Enabling refresh OS");

            recovery::mode_set("refresh", conf.default_boot())
                .map_err(|why| ReleaseError::RecoveryConf(why.into()))?;

            conf.set_default_boot_variant(&LoaderEntry::Recovery)
                .map_err(ReleaseError::SystemdBoot)?;

            Ok(true)
        }
        RefreshOp::Status => {
            info!("Checking status of refresh OS");

            recovery::mode_is("refresh")
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, FromPrimitive, PartialEq)]
pub enum UpgradeMethod {
    Offline = 1,
}

impl From<UpgradeMethod> for &'static str {
    fn from(action: UpgradeMethod) -> Self {
        match action {
            UpgradeMethod::Offline => "offline upgrade",
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, FromPrimitive, PartialEq)]
pub enum UpgradeEvent {
    UpdatingPackageLists = 1,
    FetchingPackages = 2,
    UpgradingPackages = 3,
    InstallingPackages = 4,
    UpdatingSourceLists = 5,
    FetchingPackagesForNewRelease = 6,
    AttemptingLiveUpgrade = 7,
    AttemptingSystemdUnit = 8,
    AttemptingRecovery = 9,
    Success = 10,
    SuccessLive = 11,
    Failure = 12,
    AptFilesLocked = 13,
    RemovingConflicts = 14,
    Simulating = 15,
}

impl From<UpgradeEvent> for &'static str {
    fn from(action: UpgradeEvent) -> Self {
        match action {
            UpgradeEvent::AptFilesLocked => "waiting on a process holding the apt lock files",
            UpgradeEvent::AttemptingLiveUpgrade => "attempting live upgrade to the new release",
            UpgradeEvent::AttemptingSystemdUnit => {
                "setting up the system to perform an offline upgrade on the next boot"
            }
            UpgradeEvent::AttemptingRecovery => {
                "setting up the recovery partition to install the new release"
            }
            UpgradeEvent::Failure => "an error occurred while setting up the release upgrade",
            UpgradeEvent::FetchingPackages => "fetching updated packages for the current release",
            UpgradeEvent::FetchingPackagesForNewRelease => "fetching packages for the new release",
            UpgradeEvent::InstallingPackages => {
                "ensuring that system-critical packages are installed"
            }
            UpgradeEvent::RemovingConflicts => "removing deprecated and/or conflicting packages",
            UpgradeEvent::Success => "new release is ready to install",
            UpgradeEvent::SuccessLive => "new release was successfully installed",
            UpgradeEvent::UpdatingPackageLists => "updating package lists",
            UpgradeEvent::UpdatingSourceLists => "updating the source lists",
            UpgradeEvent::UpgradingPackages => "upgrading packages for the current release",
            UpgradeEvent::Simulating => "simulating upgrade",
        }
    }
}

/// Get a list of APT URIs to fetch for this operation, and then fetch them.
pub async fn apt_fetch<H>(uris: HashSet<AptRequest, H>, func: &dyn Fn(FetchEvent)) -> RelResult<()>
where
    H: std::hash::BuildHasher,
{
    (*func)(FetchEvent::Init(uris.len()));

    apt_lock_wait().await;
    let _lock_files = hold_apt_locks()?;

    const ARCHIVES: &str = "/var/cache/apt/archives/";
    const PARTIAL: &str = "/var/cache/apt/archives/partial/";

    const CONCURRENT_FETCHES: usize = 4;
    const DELAY_BETWEEN: u64 = 100;
    const RETRIES: u32 = 3;

    let client = isahc::HttpClient::new().expect("failed to create HTTP Client");

    let (fetch_tx, fetch_rx) = flume::bounded(CONCURRENT_FETCHES);

    use apt_cmd::fetch::{EventKind, PackageFetcher};

    // The system which fetches packages we send requests to
    let mut events = PackageFetcher::new(client)
        .concurrent(CONCURRENT_FETCHES)
        .delay_between(DELAY_BETWEEN)
        .retries(RETRIES)
        .fetch(fetch_rx.into_stream(), Arc::from(Path::new(PARTIAL)));

    // The system which sends package-fetching requests
    let sender = async move {
        if !Path::new(PARTIAL).exists() {
            async_fs::create_dir_all(PARTIAL)
                .await
                .context("failed to create partial debian directory")?;
        }

        let packages = AptGet::new()
            .noninteractive()
            .fetch_uris(&["full-upgrade"])
            .await
            .context("failed to spawn apt-get command")?
            .context("failed to fetch package URIs from apt-get")?;

        for package in packages {
            info!("sending package");
            let _ = fetch_tx.send_async(Arc::new(package)).await;
            info!("sending package");
        }

        Ok::<(), anyhow::Error>(())
    };

    // The system that handles events received from the package-fetcher
    let receiver = async move {
        info!("receiving packages");
        while let Some(event) = events.next().await {
            debug!("Package Fetch Event: {:#?}", event);

            match event.kind {
                EventKind::Fetching => {
                    func(FetchEvent::Fetching((*event.package).clone()));
                }

                EventKind::Validated(src) => {
                    let dst = Path::new(ARCHIVES).join(&event.package.name);

                    async_fs::rename(&src, &dst)
                        .await
                        .context("failed to rename fetched debian package")?;

                    func(FetchEvent::Fetched((*event.package).clone()));
                }

                EventKind::Error(why) => {
                    return Err(why).context("package fetching failed");
                }

                EventKind::Fetched(_) => (),
            }
        }

        Ok::<(), anyhow::Error>(())
    };

    futures_util::try_join!(sender, receiver).map(|_| ()).map_err(ReleaseError::PackageFetch)
}

/// Check if release files can be upgraded, and then overwrite them with the new release.
///
/// On failure, the original release files will be restored.
pub async fn release_upgrade<'b>(
    logger: &dyn Fn(UpgradeEvent),
    current: &str,
    new: &str,
) -> anyhow::Result<()> {
    let current = codename_from_version(current);
    let new = codename_from_version(new);

    info!("checking if release can be upgraded from {} to {}", current, new);

    // In case the system abruptly shuts down after this point, create a file to signal
    // that packages were being fetched for a new release.
    fs::write(RELEASE_FETCH_FILE, &format!("{} {}", current, new))
        .context("failed to create release fetch file")?;

    let update_sources = async move {
        (logger)(UpgradeEvent::AptFilesLocked);

        apt_cmd::lock::apt_lock_wait().await;

        (logger)(UpgradeEvent::UpdatingPackageLists);

        repos::apply_default_source_lists(new)?;

        apt_lock_wait().await;
        AptGet::new().noninteractive().update().await.context("failed to update source lists")
    };

    if let Err(why) = update_sources.await {
        error!("failed to update sources: {}", why);

        if let Err(why) = repos::restore(current) {
            error!("failed to restore source lists: {:?}", why);
        }

        return Err(why).context("failed to update sources");
    }

    Ok(())
}

/// Upgrades packages for the current release.
pub async fn package_upgrade<C: Fn(AptUpgradeEvent)>(callback: C) -> RelResult<()> {
    let callback = &callback;

    let apt_upgrade = || async {
        apt_lock_wait().await;
        info!("upgrading packages");
        let (mut child, mut upgrade_events) =
            AptGet::new().noninteractive().allow_downgrades().force().stream_upgrade().await?;

        while let Some(event) = upgrade_events.next().await {
            callback(event);
        }

        child.status().await
    };

    apt_lock_wait().await;
    info!("autoremoving packages");
    let _ = AptGet::new().noninteractive().allow_downgrades().force().autoremove().status().await;

    // If the first upgrade attempt fails, try to dpkg --configure -a and try again.
    if apt_upgrade().await.is_err() {
        apt_lock_wait().await;
        info!("dpkg --configure -a");
        let dpkg_configure = Dpkg::new().configure_all().status().await.is_err();

        apt_lock_wait().await;
        info!("checking for broken packages");
        AptGet::new()
            .noninteractive()
            .fix_broken()
            .allow_downgrades()
            .force()
            .status()
            .await
            .map_err(ReleaseError::FixBroken)?;

        if dpkg_configure {
            apt_lock_wait().await;
            info!("dpkg --configure -a");
            Dpkg::new().configure_all().status().await.map_err(ReleaseError::DpkgConfigure)?;
        }

        apt_upgrade().await.map_err(ReleaseError::Upgrade)?;
    }

    apt_lock_wait().await;
    info!("autoremoving packages");
    let _ = AptGet::new().noninteractive().force().allow_downgrades().autoremove().status().await;

    Ok(())
}

/// Perform the release upgrade by updating release files, fetching packages required for the
/// new release, and then setting the recovery partition as the default boot entry.
#[allow(clippy::too_many_arguments)]
pub async fn upgrade<'a>(
    action: UpgradeMethod,
    from: &'a str,
    to: &'a str,
    logger: &'a dyn Fn(UpgradeEvent),
    fetch: &'a dyn Fn(FetchEvent),
    upgrade: &'a dyn Fn(AptUpgradeEvent),
) -> RelResult<()> {
    terminate_background_applications();

    let from_version = from.parse::<Version>().expect("invalid version");
    let from_codename = Codename::try_from(from_version).expect("release doesn't have a codename");

    // Ensure that prerequest files and mounts are available.
    match action {
        UpgradeMethod::Offline => systemd::upgrade_prereq()?,
    }

    let _ = AptMark::new().hold(&["pop-upgrade"]).await;

    let version = codename_from_version(from);

    // Check the system and perform any repairs necessary for success.
    (async move {
        repair::crypttab::repair().map_err(RepairError::Crypttab)?;
        repair::fstab::repair().map_err(RepairError::Fstab)?;
        repair::packaging::repair(version).await.map_err(RepairError::Packaging)?;

        Ok(())
    })
    .await
    .map_err(ReleaseError::Repair)?;

    info!("creating backup of source lists");
    repos::backup(version).map_err(ReleaseError::BackupPPAs)?;

    info!("disabling third party sources");
    repos::disable_third_parties(version).map_err(ReleaseError::DisablePPAs)?;

    if repos::is_old_release(<&'static str>::from(from_codename)) {
        info!("switching to old-releases repositories");
        repos::replace_with_old_releases().map_err(ReleaseError::OldReleaseSwitch)?;
    }

    let conflicting = (async {
        let (mut child, package_stream) = DpkgQuery::new().show_installed(REMOVE_PACKAGES).await?;

        futures_util::pin_mut!(package_stream);

        let mut packages = Vec::new();

        while let Some(package) = package_stream.next().await {
            packages.push(package);
        }

        // NOTE: This is okay to fail since it just means a package is not found
        let _ = child.status().await;

        Ok::<_, std::io::Error>(packages)
    })
    .await
    .map_err(ReleaseError::ConflictRemoval)?;

    if !conflicting.is_empty() {
        apt_lock_wait().await;
        (logger)(UpgradeEvent::RemovingConflicts);
        AptGet::new()
            .noninteractive()
            .force()
            .remove(conflicting)
            .await
            .map_err(ReleaseError::ConflictRemoval)?;
    }

    // Update the package lists for the current release.
    apt_lock_wait().await;
    (logger)(UpgradeEvent::UpdatingPackageLists);
    AptGet::new().noninteractive().update().await.map_err(ReleaseError::CurrentUpdate)?;

    // Fetch required packages for upgrading the current release.
    (*logger)(UpgradeEvent::FetchingPackages);

    let uris =
        crate::fetch::apt::fetch_uris(Some(CORE_PACKAGES)).await.map_err(ReleaseError::AptList)?;

    apt_fetch(uris, fetch).await?;

    // Upgrade the current release to the latest packages.
    (*logger)(UpgradeEvent::UpgradingPackages);
    package_upgrade(upgrade).await?;

    apt_lock_wait().await;
    (logger)(UpgradeEvent::InstallingPackages);
    AptGet::new()
        .noninteractive()
        .allow_downgrades()
        .force()
        .install(CORE_PACKAGES)
        .await
        .map_err(ReleaseError::InstallCore)?;

    // Apply any fixes necessary before the upgrade.
    repair::pre_upgrade().map_err(ReleaseError::PreUpgrade)?;

    let _ = AptMark::new().unhold(&["pop-upgrade"]).await;

    // Update the source lists to the new release,
    // then fetch the packages required for the upgrade.
    fetch_new_release_packages(logger, fetch, from, to).await?;

    if let Err(why) = crate::gnome_extensions::disable() {
        error!(
            "failed to disable gnome-shell extensions: {}",
            crate::misc::format_error(why.as_ref())
        );
    }

    (*logger)(UpgradeEvent::Success);
    Ok(())
}

/// Search for any active processes which are incompatible with the upgrade daemon,
/// and terminate them.
fn terminate_background_applications() {
    // The appcenter may fight for control over dpkg locks, and display
    // notifications.
    const APPCENTER: &str = "io.elementary.appcenter";

    let processes = match procfs::process::all_processes() {
        Ok(proc) => proc,
        Err(why) => {
            warn!("failed to fetch running processes: {}", why);
            return;
        }
    };

    for proc in processes {
        if let Ok(exe_path) = proc.exe() {
            if let Some(exe) = exe_path.file_name() {
                if let Some(mut exe) = exe.to_str() {
                    if exe.ends_with(" (deleted)") {
                        exe = &exe[..exe.len() - 10];
                    }

                    if exe == APPCENTER {
                        eprintln!("killing {}", APPCENTER);
                        unsafe {
                            let _ = libc::kill(proc.pid(), libc::SIGKILL);
                        }
                    }
                }
            }
        }
    }
}

async fn attempt_fetch<'a>(
    logger: &'a dyn Fn(UpgradeEvent),
    fetch: &'a dyn Fn(FetchEvent),
) -> RelResult<()> {
    info!("fetching packages for the new release");
    (*logger)(UpgradeEvent::FetchingPackagesForNewRelease);

    let uris = crate::fetch::apt::fetch_uris(None).await.map_err(ReleaseError::AptList)?;

    apt_fetch(uris, fetch).await
}

/// Update the release files and fetch packages for the new release.
///
/// On failure, the original release files will be restored.
async fn fetch_new_release_packages<'b>(
    logger: &'b dyn Fn(UpgradeEvent),
    fetch: &'b dyn Fn(FetchEvent),
    current: &'b str,
    next: &'b str,
) -> RelResult<()> {
    (*logger)(UpgradeEvent::UpdatingSourceLists);

    // Updates the source lists, with a handle for reverting the change.
    release_upgrade(logger, current, next).await.map_err(ReleaseError::Check)?;

    // Use a closure to capture any early returns due to an error.
    let updated_list_ops = || async {
        info!("updated the package lists for the new release");
        apt_lock_wait().await;
        (logger)(UpgradeEvent::UpdatingPackageLists);
        AptGet::new().noninteractive().update().await.map_err(ReleaseError::ReleaseUpdate)?;

        snapd::hold_transitional_packages().await?;

        attempt_fetch(logger, fetch).await?;

        info!("packages fetched successfully");

        (*logger)(UpgradeEvent::Simulating);

        simulate_upgrade().await
    };

    // On any error, roll back the source lists.
    match updated_list_ops().await {
        Ok(_) => Ok(()),
        Err(why) => {
            rollback(codename_from_version(current), &why);

            Err(why)
        }
    }
}

async fn simulate_upgrade() -> RelResult<()> {
    apt_lock_wait().await;
    AptGet::new()
        .noninteractive()
        .allow_downgrades()
        .force()
        .simulate()
        .upgrade()
        .await
        .map_err(ReleaseError::Simulation)
}

/// Currently not a supported path
pub fn upgrade_finalize(action: UpgradeMethod, from: &str, to: &str) -> RelResult<()> {
    match action {
        UpgradeMethod::Offline => systemd::upgrade_set(from, to),
    }
}

fn rollback(release: &str, why: &(dyn std::error::Error + 'static)) {
    error!("failed to fetch packages: {}", crate::misc::format_error(why));
    warn!("attempting to roll back apt release files");
    if let Err(why) = repos::restore(release) {
        error!(
            "failed to revert release name changes to source lists in /etc/apt/: {}",
            crate::misc::format_error(why.as_ref())
        );
    }
}

pub enum FetchEvent {
    Fetching(AptRequest),
    Fetched(AptRequest),
    Init(usize),
}

/// Check if certain files exist at the time of starting this daemon.
pub async fn cleanup() {
    let _ = fs::remove_file(crate::RESTART_SCHEDULED);

    let _ = AptMark::new().unhold(&["pop-upgrade"]).await;

    for &file in &[RELEASE_FETCH_FILE, STARTUP_UPGRADE_FILE] {
        if Path::new(file).exists() {
            info!("cleaning up after failed upgrade");

            match Version::detect() {
                Ok(version) => {
                    let codename = Codename::try_from(version)
                        .ok()
                        .map(<&'static str>::from)
                        .expect("no codename for version");

                    let _ = crate::release::repos::restore(codename);
                }
                Err(why) => {
                    error!("could not detect distro release version: {}", why);
                }
            }

            let _ = fs::remove_file(file);
            apt_lock_wait().await;
            let _ = AptGet::new().noninteractive().update().await;
            break;
        }
    }

    let _ = fs::remove_file(SYSTEM_UPDATE);

    if Path::new(crate::TRANSITIONAL_SNAPS).exists() {
        if let Ok(packages) = fs::read_to_string(crate::TRANSITIONAL_SNAPS) {
            for package in packages.lines() {
                let _ = AptMark::new().unhold(&[&*package]).await;
            }
        }

        let _ = fs::remove_file(crate::TRANSITIONAL_SNAPS);
    }
}

fn hold_apt_locks() -> RelResult<(File, File)> {
    File::open(LISTS_LOCK)
        .and_then(|lists| File::open(DPKG_LOCK).map(|dpkg| (lists, dpkg)))
        .map_err(ReleaseError::Lock)
}

fn codename_from_version(version: &str) -> &str {
    version
        .parse::<Version>()
        .ok()
        .and_then(|x| Codename::try_from(x).ok())
        .map(<&'static str>::from)
        .unwrap_or(version)
}
