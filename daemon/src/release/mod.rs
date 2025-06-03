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
use crate::{
    fetch::apt::ExtraPackages,
    repair::{self, RepairError},
};

use crate::ubuntu_version::{Codename, Version};
use anyhow::Context;
use apt_cmd::{
    lock::apt_lock_wait, request::Request as AptRequest, AptGet, AptMark, AptUpgradeEvent, Dpkg,
    DpkgQuery,
};
use async_shutdown::ShutdownManager as Shutdown;
use futures::prelude::*;
use std::{
    collections::HashSet,
    convert::TryFrom,
    fs::{self, File},
    os::unix::fs::symlink,
    path::Path,
    sync::Arc,
};
use systemd_boot_conf::SystemdBootConf;

pub const STARTUP_UPGRADE_FILE: &str = "/pop-upgrade";

/// Packages which should be removed before upgrading.
///
/// - `gnome-software` conflicts with `pop-desktop` and its `sessioninstaller` dependency
/// - `ureadahead` was deprecated and removed from the repositories
/// - `update-notifier-common` breaks debconf and it's not part of a Pop install
/// - `nodejs` may have some dependencies which are unmet on 22.04
const REMOVE_PACKAGES: &[&str] = &[
    "irqbalance",
    "ureadahead",
    "backport-iwlwifi-dkms",
    "update-notifier-common",
    "nodejs",
    "ttf-mscorefonts-installer",
];

/// Packages which should be installed before upgrading.
///
/// - `linux-generic` because some systems may have a different kernel installed
/// - `pop-desktop` because it pulls in all of our required desktop dependencies
/// - `sessioninstaller` because it may have been removed by `gnome-software`
#[cfg(target_arch = "x86_64")]
const CORE_PACKAGES: &[&str] = &["linux-generic", "pop-desktop", "sessioninstaller"];

#[cfg(target_arch = "aarch64")]
const CORE_PACKAGES: &[&str] = &["pop-desktop-raspi"];

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
    FetchingAdditionalPackagesForNewRelease = 7,
    AttemptingLiveUpgrade = 8,
    AttemptingSystemdUnit = 9,
    AttemptingRecovery = 10,
    Success = 11,
    SuccessLive = 12,
    Failure = 13,
    AptFilesLocked = 14,
    RemovingConflicts = 15,
    Simulating = 16,
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
            UpgradeEvent::FetchingPackagesForNewRelease => "fetching updated packages for the new release",
            UpgradeEvent::FetchingAdditionalPackagesForNewRelease => "fetching additional packages for the new release",
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
pub async fn apt_fetch(
    shutdown: Shutdown<()>,
    mut uris: HashSet<AptRequest, std::collections::hash_map::RandomState>,
    func: &dyn Fn(FetchEvent),
) -> RelResult<()> {
    apt_lock_wait().await;
    let _lock_files = hold_apt_locks()?;

    let task = async {
        let mut result = Ok(HashSet::new());

        for _ in 0..3 {
            (*func)(FetchEvent::Init(uris.len()));

            result = apt_fetch_(shutdown.clone(), uris.clone(), func).await;

            match result.as_mut() {
                Ok(borrowed) => {
                    let mut errored = HashSet::new();

                    std::mem::swap(&mut errored, borrowed);
                    if errored.is_empty() {
                        break;
                    }

                    uris = errored;
                }
                Err(why) => {
                    error!("package fetching failed: {}", why);
                }
            }
        }

        result.map(|_| ())
    };

    let cancel = async {
        let _ = shutdown.wait_shutdown_triggered().await;
        info!("canceled download");
        Err(ReleaseError::Canceled)
    };

    futures::pin_mut!(task);
    futures::pin_mut!(cancel);

    let result = future::select(cancel, task).await.factor_first().0;

    result
}

async fn apt_fetch_(
    shutdown: Shutdown<()>,
    uris: HashSet<AptRequest, std::collections::hash_map::RandomState>,
    func: &dyn Fn(FetchEvent),
) -> Result<HashSet<AptRequest, std::collections::hash_map::RandomState>, ReleaseError> {
    const ARCHIVES: &str = "/var/cache/apt/archives/";
    const PARTIAL: &str = "/var/cache/apt/archives/partial/";

    let (fetch_tx, fetch_rx) = flume::unbounded();

    use apt_cmd::fetch::{EventKind, FetcherExt};

    let mut errored = HashSet::new();

    // The system which fetches packages we send requests to
    let (fetcher, mut events) = async_fetcher::Fetcher::default()
        .retries(3)
        .connections_per_file(1)
        .timeout(std::time::Duration::from_secs(15))
        .shutdown(shutdown.clone())
        .into_package_fetcher()
        .concurrent(2)
        .fetch(fetch_rx.into_stream(), Arc::from(Path::new(ARCHIVES)));

    tokio::spawn(fetcher);

    // The system which sends package-fetching requests
    let sender = tokio::spawn(async move {
        if !Path::new(PARTIAL).exists() {
            tokio::fs::create_dir_all(PARTIAL)
                .await
                .context("failed to create partial debian directory")?;
        }

        for request in uris {
            let _ = fetch_tx.send_async(Arc::new(request)).await;
        }

        Ok::<(), anyhow::Error>(())
    });

    let sender = async move { sender.await.unwrap() };

    // The system that handles events received from the package-fetcher
    let receiver = async {
        while let Some(event) = events.recv().await {
            match event.kind {
                EventKind::Fetching => {
                    func(FetchEvent::Fetching((*event.package.uri).to_owned()));
                }

                EventKind::Validated => {
                    func(FetchEvent::Fetched((*event.package).clone()));
                }

                EventKind::Error(why) => {
                    error!("{}: fetch error: {:?}", event.package.name, why);
                    errored.insert(event.package.as_ref().clone());
                }

                EventKind::Fetched => (),

                EventKind::Retrying => {
                    info!("{}: retrying fetch", event.package.name);
                    func(FetchEvent::Retrying((*event.package).clone()));
                }
            }
        }

        Ok::<(), anyhow::Error>(())
    };

    let _ = futures::try_join!(sender, receiver).map(|_| ()).map_err(ReleaseError::PackageFetch)?;
    Ok(errored)
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

        apt_lock_wait().await;

        (logger)(UpgradeEvent::UpdatingPackageLists);

        repos::apply_default_source_lists(new).await?;

        apt_lock_wait().await;
        AptGet::new().noninteractive().update().await.context("failed to update source lists")
    };

    if let Err(why) = update_sources.await {
        error!("failed to update sources: {}", why);

        if let Err(why) = repos::restore(current).await {
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
        let (mut child, mut upgrade_events) = crate::misc::apt_get().stream_upgrade().await?;

        while let Some(event) = upgrade_events.next().await {
            callback(event);
        }

        child.wait().await
    };

    apt_lock_wait().await;
    info!("autoremoving packages");
    let _ = crate::misc::apt_get().autoremove().status().await;

    // If the first upgrade attempt fails, try to dpkg --configure -a and try again.
    if apt_upgrade().await.is_err() {
        apt_lock_wait().await;
        info!("dpkg --configure -a");
        let dpkg_configure = Dpkg::new().configure_all().status().await.is_err();

        apt_lock_wait().await;
        info!("checking for broken packages");
        crate::misc::apt_get().fix_broken().status().await.map_err(ReleaseError::FixBroken)?;

        if dpkg_configure {
            apt_lock_wait().await;
            info!("dpkg --configure -a");
            Dpkg::new()
                .force_confdef()
                .force_confold()
                .configure_all()
                .status()
                .await
                .map_err(ReleaseError::DpkgConfigure)?;
        }

        apt_upgrade().await.map_err(ReleaseError::Upgrade)?;
    }

    apt_lock_wait().await;
    info!("autoremoving packages");
    let _ = crate::misc::apt_get().autoremove().status().await;

    Ok(())
}

/// Perform the release upgrade by updating release files, fetching packages required for the
/// new release, and then setting the recovery partition as the default boot entry.
#[allow(clippy::too_many_arguments)]
pub async fn upgrade<'a>(
    _action: UpgradeMethod,
    from: &'a str,
    to: &'a str,
    logger: &'a dyn Fn(UpgradeEvent),
    fetch: &'a dyn Fn(FetchEvent),
    upgrade: &'a dyn Fn(AptUpgradeEvent),
) -> RelResult<()> {
    terminate_background_applications();

    // Unhold all held packages
    unhold_all().await;

    let from_version = from.parse::<Version>().expect("invalid version");
    let from_codename = Codename::try_from(from_version).expect("release doesn't have a codename");

    // Ensure that prerequest files and mounts are available.
    systemd::upgrade_prereq()?;

    let _ = AptMark::new().hold(&["pop-upgrade", "pop-system-updater"]).await;

    let version = codename_from_version(from);

    // Check the system and perform any repairs necessary for success.
    autorepair(version).await?;

    info!("creating backup of source lists");
    repos::backup(version).await.map_err(ReleaseError::BackupPPAs)?;

    // Old releases need a workaround to change their source URIs.
    old_releases_workaround(version, from_codename).await?;

    // Update the current release's package lists.
    (logger)(UpgradeEvent::UpdatingPackageLists);
    update_package_lists().await?;

    // Upgrade the current release to the latest packages.
    (*logger)(UpgradeEvent::UpgradingPackages);
    fetch_current_updates(fetch).await?;

    // Fetch required packages for upgrading the current release.
    (*logger)(UpgradeEvent::FetchingPackages);
    package_upgrade(upgrade).await?;

    // Ensure packages are not newer than what's in the repositories.
    downgrade_packages().await?;

    // Remove packages that may conflict with the upgrade.
    remove_conflicting_packages(logger).await?;

    (logger)(UpgradeEvent::InstallingPackages);
    install_essential_packages().await?;

    // Apply any fixes necessary before the upgrade.
    repair::pre_upgrade().map_err(ReleaseError::PreUpgrade)?;
    let _ = AptMark::new().unhold(&["pop-upgrade", "pop-system-updater"]).await;

    // Upgrade the apt sources to the new release.
    (*logger)(UpgradeEvent::UpdatingSourceLists);
    release_upgrade(logger, from, to).await.map_err(ReleaseError::Check)?;

    // Update lists and fetch packages for the new release.
    fetch_new_release_packages(logger, fetch, from, to).await?;

    // Remove packages that are orphaned in the new release

    if let Err(why) = crate::gnome_extensions::disable() {
        error!(
            "failed to disable gnome-shell extensions: {}",
            crate::misc::format_error(why.as_ref())
        );
    }

    (*logger)(UpgradeEvent::Success);
    Ok(())
}

async fn autorepair(version: &str) -> Result<(), ReleaseError> {
    (async move {
        repair::crypttab::repair().map_err(RepairError::Crypttab)?;
        repair::fstab::repair().map_err(RepairError::Fstab)?;
        repair::packaging::repair(version).await.map_err(RepairError::Packaging)?;

        Ok(())
    })
    .await
    .map_err(ReleaseError::Repair)
}

async fn downgrade_packages() -> Result<(), ReleaseError> {
    info!("searching for packages that require downgrading");
    let downgradable =
        apt_cmd::apt::downgradable_packages().await.map_err(ReleaseError::Downgrade)?;

    let mut cmd = AptGet::new().allow_downgrades().force().noninteractive();

    cmd.arg("install");

    'downgrades: for (package, version) in &downgradable {
        if package.contains("pop-upgrade") || package.contains("pop-system-updater") {
            continue;
        }
        
        // Papirus's elementary variant must be removed prior to downgrading the main package.
        if package.contains("papirus-icon-theme") {
            info!("papirus-icon-theme will be downgraded, so removing epapirus-icon-theme");
            let mut remove_epapirus_cmd = AptGet::new().allow_downgrades().force().noninteractive();
            remove_epapirus_cmd.arg("remove");
            remove_epapirus_cmd.arg("epapirus-icon-theme");
            let _remove_epapirus = remove_epapirus_cmd.status().await
                .context("apt-get remove epapirus-icon-theme").map_err(ReleaseError::Downgrade);
        }
        
        // In Ubuntu 22.04, the `ansible` and `ansible-core` packages are not compatible.
        // If `ansible-core` is downgradable, check if `ansible` is downgradable;
        // if so, remove `ansible-core` and skip adding it to the downgrade command.
        if package.contains("ansible-core") && version.contains("2.12") {
            info!("ansible-core downgrade candidate version is from Ubuntu 22.04");
            for (package, _version) in &downgradable {
                if package.eq("ansible") {
                    info!("ansible will also be downgraded, so removing ansible-core");
                    let mut remove_ansible_core_cmd = AptGet::new().allow_downgrades().force().noninteractive();
                        remove_ansible_core_cmd.arg("remove");
                        remove_ansible_core_cmd.arg("ansible-core");
                        let _remove_ansible_core = remove_ansible_core_cmd.status().await
                            .context("apt-get remove ansible-core").map_err(ReleaseError::Downgrade);
                    continue 'downgrades;
                }
            }
            info!("ansible is not installed, so ansible-core will be downgraded");
        }

        if let Some(version) = version.split_ascii_whitespace().next() {
            cmd.arg([&package, "=", version].concat());
        }
    }

    info!("downgrading packages with: {:?}", cmd.as_std());
    cmd.status().await.context("apt-get downgrade").map_err(ReleaseError::Downgrade).map(drop)
}

async fn install_essential_packages() -> Result<(), ReleaseError> {
    apt_lock_wait().await;
    crate::misc::apt_get().install(CORE_PACKAGES).await.map_err(ReleaseError::InstallCore)
}

/// Update the package lists for the current release.
async fn update_package_lists() -> Result<(), ReleaseError> {
    apt_lock_wait().await;
    AptGet::new().noninteractive().update().await.map_err(ReleaseError::CurrentUpdate)
}

/// Fetch apt packages and retry if network connections are changed.
async fn fetch_current_updates(fetch: &dyn Fn(FetchEvent)) -> Result<(), ReleaseError> {
    let packages = Some(ExtraPackages::Static(CORE_PACKAGES));
    let uris = crate::fetch::apt::fetch_uris(Shutdown::new(), packages, true)
        .await
        .map_err(ReleaseError::AptList)?;

    apt_fetch(Shutdown::new(), uris, fetch).await
}

async fn old_releases_workaround(version: &str, codename: Codename) -> Result<(), ReleaseError> {
    info!("disabling third party sources");
    repos::disable_third_parties(version).await.map_err(ReleaseError::DisablePPAs)?;

    if repos::is_old_release(<&'static str>::from(codename)).await {
        info!("switching to old-releases repositories");
        repos::replace_with_old_releases().map_err(ReleaseError::OldReleaseSwitch)?;
    }

    Ok(())
}

async fn remove_conflicting_packages(logger: &dyn Fn(UpgradeEvent)) -> Result<(), ReleaseError> {
    let mut conflicting = (async {
        let (mut child, package_stream) = DpkgQuery::new().show_installed(REMOVE_PACKAGES).await?;

        futures_util::pin_mut!(package_stream);

        let mut packages = Vec::new();

        while let Some(package) = package_stream.next().await {
            packages.push(package);
        }

        // NOTE: This is okay to fail since it just means a package is not found
        let _ = child.wait().await;

        Ok::<_, std::io::Error>(packages)
    })
    .await
    .context("check for known-conflicting packages")
    .map_err(ReleaseError::ConflictRemoval)?;

    // Add packages which have no remote to the conflict list
    if let Ok(mut packages) = apt_cmd::apt::remoteless_packages().await {
        // Add exemptions for specific packages that we know to be safe.
        packages.retain(|name| name != "sentinelagent");
        // Add the packages that remain to our list of conflicting packages for removal.
        conflicting.extend_from_slice(&packages);
    }

    if !conflicting.is_empty() {
        apt_lock_wait().await;
        (logger)(UpgradeEvent::RemovingConflicts);
        let mut apt_get = crate::misc::apt_get();

        apt_get.arg("--auto-remove");
        apt_get
            .remove(conflicting)
            .await
            .context("conflict removal")
            .map_err(ReleaseError::ConflictRemoval)?;
    }

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
        if let Ok(proc) = proc {
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

    let _ = std::process::Command::new("systemctl")
        .arg("stop")
        .arg("com.system76.SystemUpdater")
        .status();
}

async fn attempt_fetch<'a>(
    shutdown: &Shutdown<()>,
    logger: &'a dyn Fn(UpgradeEvent),
    fetch: &'a dyn Fn(FetchEvent),
) -> RelResult<()> {
    info!("fetching updated packages for the new release");
    (*logger)(UpgradeEvent::FetchingPackagesForNewRelease);

    let uris = crate::fetch::apt::fetch_uris(shutdown.clone(), None, true)
        .await
        .map_err(ReleaseError::AptList)?;

    apt_fetch(shutdown.clone(), uris, fetch).await
}

async fn additional_fetch<'a>(
    shutdown: &Shutdown<()>,
    logger: &'a dyn Fn(UpgradeEvent),
    fetch: &'a dyn Fn(FetchEvent),
    new_packages: &'static [&str],
) -> RelResult<()> {
    info!("fetching additional packages for the new release");
    (*logger)(UpgradeEvent::FetchingAdditionalPackagesForNewRelease);

    let packages = Some(ExtraPackages::Static(new_packages));
    let uris = crate::fetch::apt::fetch_uris(shutdown.clone(), packages, false)
        .await
        .map_err(ReleaseError::AptList)?;

    apt_fetch(shutdown.clone(), uris, fetch).await
}

/// Fetch packages for the new release.
///
/// On failure, the original release files will be restored.
async fn fetch_new_release_packages<'b>(
    logger: &'b dyn Fn(UpgradeEvent),
    fetch: &'b dyn Fn(FetchEvent),
    current: &'b str,
    to: &'b str,
) -> RelResult<()> {
    // Use a closure to capture any early returns due to an error.
    let updated_list_ops = || async {
        info!("updating the package lists for the new release");
        apt_lock_wait().await;
        (logger)(UpgradeEvent::UpdatingPackageLists);
        AptGet::new().noninteractive().update().await.map_err(ReleaseError::ReleaseUpdate)?;

        attempt_fetch(&Shutdown::new(), logger, fetch).await?;

        // If upgrading to 24.04, download an additional package.
        if to == "24.04" {
            //const NEW_PACKAGES &[&str] = &["gnome-online-accounts-gtk"];
            //new_packages = Some(ExtraPackages::Static(NEW_PACKAGES));
            additional_fetch(&Shutdown::new(), logger, fetch, &["gnome-online-accounts-gtk"]).await?;
        }

        snapd::hold_transitional_packages().await?;

        info!("packages fetched successfully");

        (*logger)(UpgradeEvent::Simulating);

        simulate_upgrade().await
    };

    // On any error, roll back the source lists.
    match updated_list_ops().await {
        Ok(_) => Ok(()),
        Err(why) => {
            rollback(codename_from_version(current), &why).await;

            Err(why)
        }
    }
}

async fn simulate_upgrade() -> RelResult<()> {
    apt_lock_wait().await;
    crate::misc::apt_get().simulate().upgrade().await.map_err(ReleaseError::Simulation)
}

pub fn upgrade_finalize(action: UpgradeMethod, from: &str, to: &str) -> RelResult<()> {
    // Ensure that the splash kernel option is enabled in case it was removed.
    // This kernel option is required to show the Plymouth splash on next boot.
    if Path::new("/usr/bin/kernelstub").exists() {
        let _res = std::process::Command::new("kernelstub").arg("-a").arg("splash").status();
    }

    match action {
        UpgradeMethod::Offline => systemd::upgrade_set(from, to),
    }
}

async fn rollback(release: &str, why: &(dyn std::error::Error + 'static)) {
    error!("failed to fetch packages: {}", crate::misc::format_error(why));
    warn!("attempting to roll back apt release files");
    if let Err(why) = repos::restore(release).await {
        error!(
            "failed to revert release name changes to source lists in /etc/apt/: {}",
            crate::misc::format_error(why.as_ref())
        );
    }
}

pub enum FetchEvent {
    Fetching(String),
    Fetched(AptRequest),
    Init(usize),
    Retrying(AptRequest),
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

/// apt-mark unhold all held packages.
async fn unhold_all() {
    if let Ok(output) = tokio::process::Command::new("apt-mark").arg("showhold").output().await {
        if let Ok(output) = String::from_utf8(output.stdout) {
            let mut packages = Vec::new();
            for line in output.lines() {
                packages.push(line);
            }

            let _ = AptMark::new().unhold(&packages).await;
        }
    }
}
