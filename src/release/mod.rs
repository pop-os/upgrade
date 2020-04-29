pub mod check;
pub mod eol;

mod errors;
mod snapd;

pub use self::{
    check::{BuildStatus, ReleaseStatus},
    errors::{RelResult, ReleaseError},
};

use apt_fetcher::{
    apt_uris::{apt_uris, AptUri},
    SourcesLists, UpgradeRequest, Upgrader,
};
use envfile::EnvFile;
use futures::{stream, Future, Stream};
use std::{
    collections::HashSet,
    convert::TryFrom,
    fs::{self, File},
    os::unix::fs::symlink,
    path::Path,
    sync::Arc,
};
use systemd_boot_conf::SystemdBootConf;

use crate::{daemon::DaemonRuntime, repair};
use apt_cli_wrappers::*;
use ubuntu_version::{Codename, Version};

pub const STARTUP_UPGRADE_FILE: &str = "/pop-upgrade";

const REQUIRED_PPAS: &[&str] = &[
    "archive.ubuntu.com/ubuntu",
    "ppa.launchpad.net/system76/pop/ubuntu",
    "apt.pop-os.org/proprietary",
];

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

pub fn is_required_ppa(ppa: &str) -> bool {
    REQUIRED_PPAS.iter().any(|&required| ppa.contains(required))
}

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
    recovery_prereq()?;

    let action = match op {
        RefreshOp::Disable => unset_recovery_as_default_boot_option,
        RefreshOp::Enable => set_recovery_as_default_boot_option,
        RefreshOp::Status => get_recovery_value_set,
    };

    action("REFRESH")
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, FromPrimitive, PartialEq)]
pub enum UpgradeMethod {
    Offline = 1,
    Recovery = 2,
}

impl From<UpgradeMethod> for &'static str {
    fn from(action: UpgradeMethod) -> Self {
        match action {
            UpgradeMethod::Offline => "offline upgrade",
            UpgradeMethod::Recovery => "recovery partition",
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
            UpgradeEvent::UpdatingPackageLists => "updating package lists for the current release",
            UpgradeEvent::UpdatingSourceLists => "updating the source lists to the new release",
            UpgradeEvent::UpgradingPackages => "upgrading packages for the current release",
        }
    }
}

impl<'a> DaemonRuntime<'a> {
    /// Get a list of APT URIs to fetch for this operation, and then fetch them.
    pub fn apt_fetch(
        &mut self,
        uris: HashSet<AptUri>,
        func: Arc<dyn Fn(FetchEvent) + Send + Sync>,
    ) -> RelResult<()> {
        (*func)(FetchEvent::Init(uris.len()));
        let _lock_files = hold_apt_locks()?;

        // Try to download packages until three attempts have failed.
        let mut tries = 0;

        loop {
            let stream_of_downloads = stream::iter_ok(uris.clone());
            let buffered_stream = stream_of_downloads
                .map(enclose!((func, self.client => client) move |uri| {
                    func(FetchEvent::Fetching(uri.clone()));
                    uri.fetch(&client)
                }))
                .buffer_unordered(8)
                .for_each(enclose!((func) move |uri| {
                    func(FetchEvent::Fetched(uri));
                    Ok(())
                }))
                .map_err(|(uri, why)| ReleaseError::PackageFetch(uri.name, uri.uri, why));

            match self.runtime.block_on(buffered_stream) {
                Ok(_) => break Ok(()),
                Err(why) => {
                    tries += 1;
                    if tries == 3 {
                        break Err(why);
                    }

                    eprintln!("retrying apt fetching after error occurred: {}", why);
                }
            }
        }
    }

    /// Check if release files can be upgraded, and then overwrite them with the new release.
    ///
    /// On failure, the original release files will be restored.
    pub fn release_upgrade<'b>(
        &mut self,
        retain: &'b HashSet<Box<str>>,
        current: &str,
        new: &str,
    ) -> Result<Upgrader<'b>, ReleaseError> {
        let _lock_files = hold_apt_locks()?;
        let current = current
            .parse::<Version>()
            .ok()
            .and_then(|x| Codename::try_from(x).ok())
            .map(<&'static str>::from)
            .unwrap_or(current);

        let new = new
            .parse::<Version>()
            .ok()
            .and_then(|x| Codename::try_from(x).ok())
            .map(<&'static str>::from)
            .unwrap_or(new);

        let sources = SourcesLists::scan().unwrap();

        info!("checking if release can be upgraded from {} to {}", current, new);
        let request = UpgradeRequest::new(self.client.clone(), sources, self.runtime);
        let mut upgrade = request.send(retain, current, new).map_err(ReleaseError::Check)?;

        // In case the system abruptly shuts down after this point, create a file to signal
        // that packages were being fetched for a new release.
        fs::write(RELEASE_FETCH_FILE, &format!("{} {}", current, new))
            .map_err(ReleaseError::ReleaseFetchFile)?;

        info!("upgrade is possible -- updating release files");
        upgrade.overwrite_apt_sources().map_err(ReleaseError::Overwrite)?;

        Ok(upgrade)
    }

    /// Upgrades packages for the current release.
    pub fn package_upgrade<C: Fn(AptUpgradeEvent)>(&mut self, callback: C) -> RelResult<()> {
        let callback = &callback;
        let on_lock = &|ready: bool| {
            if !ready {
                (*callback)(AptUpgradeEvent::WaitingOnLock)
            }
        };

        let _ = apt_autoremove(on_lock);

        // If the first upgrade attempt fails, try to dpkg --configure -a and try again.
        if apt_upgrade(callback).is_err() {
            let dpkg_configure = dpkg_configure_all(on_lock).is_err();
            apt_install_fix_broken(on_lock).map_err(ReleaseError::FixBroken)?;

            if dpkg_configure {
                dpkg_configure_all(on_lock).map_err(ReleaseError::DpkgConfigure)?
            }

            apt_upgrade(callback).map_err(ReleaseError::Upgrade)?;
        }

        let _ = apt_autoremove(on_lock);

        Ok(())
    }

    /// Perform the release upgrade by updating release files, fetching packages required for the
    /// new release, and then setting the recovery partition as the default boot entry.
    #[allow(clippy::too_many_arguments)]
    pub fn upgrade(
        &mut self,
        action: UpgradeMethod,
        from: &str,
        to: &str,
        retain: &HashSet<Box<str>>,
        logger: &dyn Fn(UpgradeEvent),
        fetch: Arc<dyn Fn(FetchEvent) + Send + Sync>,
        upgrade: &dyn Fn(AptUpgradeEvent),
    ) -> RelResult<()> {
        self.terminate_background_applications();

        // Check the system and perform any repairs necessary for success.
        repair::repair().map_err(ReleaseError::Repair)?;

        let lock_or = |ready, then: UpgradeEvent| {
            (*logger)(if ready { then } else { UpgradeEvent::AptFilesLocked })
        };

        // Ensure that prerequest files and mounts are available.
        match action {
            UpgradeMethod::Recovery => recovery_prereq()?,
            UpgradeMethod::Offline => Self::systemd_upgrade_prereq_check()?,
        }

        let string_buffer = &mut String::new();
        let conflicting = installed(string_buffer, REMOVE_PACKAGES);
        apt_remove(conflicting, |ready| lock_or(ready, UpgradeEvent::RemovingConflicts))
            .map_err(ReleaseError::ConflictRemoval)?;

        // Update the package lists for the current release.
        apt_update(|ready| lock_or(ready, UpgradeEvent::UpdatingPackageLists))
            .map_err(ReleaseError::CurrentUpdate)?;

        // Fetch required packages for upgrading the current release.
        (*logger)(UpgradeEvent::FetchingPackages);
        let mut uris = apt_uris(&["full-upgrade"]).map_err(ReleaseError::AptList)?;

        // Also include the packages which we must have installed.
        let install_uris = apt_uris(&{
            let mut args = vec!["install"];
            args.extend_from_slice(CORE_PACKAGES);
            args
        })
        .map_err(ReleaseError::AptList)?;

        for uri in install_uris {
            uris.insert(uri);
        }

        self.apt_fetch(uris, fetch.clone())?;

        // Upgrade the current release to the latest packages.
        (*logger)(UpgradeEvent::UpgradingPackages);
        self.package_upgrade(upgrade)?;

        // Install any packages that are deemed critical.
        apt_install(CORE_PACKAGES, |ready| lock_or(ready, UpgradeEvent::InstallingPackages))
            .map_err(ReleaseError::InstallCore)?;

        // Apply any fixes necessary before the upgrade.
        repair::pre_upgrade().map_err(ReleaseError::PreUpgrade)?;

        // Update the source lists to the new release,
        // then fetch the packages required for the upgrade.
        let _ = self.fetch_new_release_packages(logger, retain, fetch, from, to)?;

        (*logger)(UpgradeEvent::Success);
        Ok(())
    }

    /// Search for any active processes which are incompatible with the upgrade daemon,
    /// and terminate them.
    fn terminate_background_applications(&mut self) {
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

    /// Validate that the pre-required files for performing a system upgrade are in place.
    fn systemd_upgrade_prereq_check() -> RelResult<()> {
        const REQUIRED_UPGRADE_FILES: [&str; 3] = [
            "/usr/lib/pop-upgrade/upgrade.sh",
            "/usr/lib/systemd/system/pop-upgrade-init.service",
            "/usr/lib/systemd/system/system-update.target.wants/pop-upgrade-init.service",
        ];

        let invalid = REQUIRED_UPGRADE_FILES
            .iter()
            .cloned()
            .filter(|file| !Path::new(file).is_file())
            .collect::<Vec<&'static str>>();

        if !invalid.is_empty() {
            return Err(ReleaseError::SystemdUpgradeFilesMissing(invalid));
        }

        Ok(())
    }

    fn attempt_fetch(
        &mut self,
        logger: &dyn Fn(UpgradeEvent),
        fetch: Arc<dyn Fn(FetchEvent) + Send + Sync>,
    ) -> RelResult<()> {
        info!("fetching packages for the new release");
        (*logger)(UpgradeEvent::FetchingPackagesForNewRelease);
        let uris = apt_uris(&["full-upgrade"]).map_err(ReleaseError::AptList)?;
        self.apt_fetch(uris, fetch)?;

        Ok(())
    }

    /// Update the release files and fetch packages for the new release.
    ///
    /// On failure, the original release files will be restored.
    fn fetch_new_release_packages<'b>(
        &mut self,
        logger: &dyn Fn(UpgradeEvent),
        retain: &'b HashSet<Box<str>>,
        fetch: Arc<dyn Fn(FetchEvent) + Send + Sync>,
        current: &str,
        next: &str,
    ) -> RelResult<Upgrader<'b>> {
        (*logger)(UpgradeEvent::UpdatingSourceLists);
        // Updates the source lists, with a handle for reverting the change.
        let mut upgrader = self.release_upgrade(retain, &current, &next)?;

        // Use a closure to capture any early returns due to an error.
        let updated_list_ops = || {
            info!("updated the package lists for the new relaese");
            apt_update(|ready| {
                (*logger)(if ready {
                    UpgradeEvent::UpdatingPackageLists
                } else {
                    UpgradeEvent::AptFilesLocked
                })
            })
            .map_err(ReleaseError::ReleaseUpdate)?;

            snapd::hold_transitional_packages()?;

            self.attempt_fetch(logger, fetch)?;

            info!("packages fetched successfully");

            Ok(())
        };

        // On any error, roll back the source lists.
        match updated_list_ops() {
            Ok(_) => Ok(upgrader),
            Err(why) => {
                rollback(&mut upgrader, &why);

                Err(why)
            }
        }
    }
}

pub fn upgrade_finalize(action: UpgradeMethod, from: &str, to: &str) -> RelResult<()> {
    match action {
        UpgradeMethod::Offline => systemd_upgrade_set(from, to),
        UpgradeMethod::Recovery => set_recovery_as_default_boot_option("UPGRADE").map(|_| ()),
    }
}

fn rollback<E: ::std::fmt::Display>(upgrader: &mut Upgrader, why: &E) {
    error!("failed to fetch packages: {}", why);
    warn!("attempting to roll back apt release files");
    if let Err(why) = upgrader.revert_apt_sources() {
        error!("failed to revert release name changes to source lists in /etc/apt/: {}", why);
    }
}

/// Create the system upgrade files that systemd will check for at startup.
fn systemd_upgrade_set(from: &str, to: &str) -> RelResult<()> {
    let current = from
        .parse::<Version>()
        .ok()
        .and_then(|x| Codename::try_from(x).ok())
        .map(<&'static str>::from)
        .unwrap_or(from);

    let new = to
        .parse::<Version>()
        .ok()
        .and_then(|x| Codename::try_from(x).ok())
        .map(<&'static str>::from)
        .unwrap_or(to);

    fs::write(STARTUP_UPGRADE_FILE, &format!("{} {}", current, new))
        .and_then(|_| symlink("/var/cache/apt/archives", SYSTEM_UPDATE))
        .map_err(ReleaseError::StartupFileCreation)
}

fn get_recovery_value_set(option: &str) -> RelResult<bool> {
    Ok(EnvFile::new(Path::new("/recovery/recovery.conf"))
        .map_err(ReleaseError::RecoveryConfOpen)?
        .get(option)
        .unwrap_or("0")
        == "1")
}

enum LoaderEntry {
    Current,
    Recovery,
}

/// Fetch the systemd-boot configuration, and designate the recovery partition as the default
/// boot option.
///
/// It will be up to the recovery partition to revert this change once it has completed its job.
fn set_recovery_as_default_boot_option(option: &str) -> RelResult<bool> {
    systemd_boot_loader_swap(LoaderEntry::Recovery, "recovery partition")?;

    EnvFile::new(Path::new("/recovery/recovery.conf"))
        .map_err(ReleaseError::RecoveryConfOpen)?
        .update(option, "1")
        .write()
        .map_err(ReleaseError::RecoveryUpdate)?;

    Ok(true)
}

fn unset_recovery_as_default_boot_option(option: &str) -> RelResult<bool> {
    systemd_boot_loader_swap(LoaderEntry::Current, "os partition")?;

    let mut envfile = EnvFile::new(Path::new("/recovery/recovery.conf"))
        .map_err(ReleaseError::RecoveryConfOpen)?;

    // TODO: Add a convenience method to envfile.
    envfile.store.remove(option);

    envfile.write().map_err(ReleaseError::RecoveryUpdate)?;
    Ok(false)
}

fn systemd_boot_loader_swap(loader: LoaderEntry, description: &str) -> RelResult<()> {
    info!("gathering systemd-boot configuration information");

    let mut systemd_boot_conf =
        SystemdBootConf::new("/boot/efi").map_err(ReleaseError::SystemdBootConf)?;

    {
        info!("found the systemd-boot config -- searching for the {}", description);
        let SystemdBootConf { ref entries, ref mut loader_conf, .. } = systemd_boot_conf;
        let recovery_entry = entries
            .iter()
            .find(|e| match loader {
                LoaderEntry::Current => e.id.to_lowercase().ends_with("current"),
                LoaderEntry::Recovery => e.id.to_lowercase().starts_with("recovery"),
            })
            .ok_or(ReleaseError::MissingRecoveryEntry)?;

        loader_conf.default = Some(recovery_entry.id.to_owned());
    }

    info!("found the {} -- setting it as the default boot entry", description);
    systemd_boot_conf.overwrite_loader_conf().map_err(ReleaseError::SystemdBootConfOverwrite)
}

pub enum FetchEvent {
    Fetching(AptUri),
    Fetched(AptUri),
    Init(usize),
}

/// Check if certain files exist at the time of starting this daemon.
pub fn cleanup() {
    for &file in [RELEASE_FETCH_FILE, STARTUP_UPGRADE_FILE].iter() {
        if let Ok(data) = fs::read_to_string(file) {
            info!("cleaning up after {} ({})", file, data);
            let mut iter = data.split_whitespace();
            if let (Some(current), Some(next)) = (iter.next(), iter.next()) {
                info!("current: {}; next: {}", current, next);
                if let Ok(mut lists) = SourcesLists::scan() {
                    info!("found lists");
                    lists.dist_replace(next, current);
                    let _ = lists.write_sync();
                }
            }

            let _ = fs::remove_file(file);
            let _ = apt_update(|ready| {
                if !ready {
                    info!("waiting for apt lock files to be free");
                }
            });
            break;
        }
    }

    let _ = fs::remove_file(SYSTEM_UPDATE);

    if Path::new(crate::TRANSITIONAL_SNAPS).exists() {
        if let Ok(packages) = fs::read_to_string(crate::TRANSITIONAL_SNAPS) {
            for package in packages.lines() {
                let _ = apt_unhold(&*package);
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

fn recovery_prereq() -> RelResult<()> {
    if !Path::new(SYSTEMD_BOOT_LOADER).exists() {
        return Err(ReleaseError::SystemdBootLoaderNotFound);
    }

    if !Path::new(SYSTEMD_BOOT_LOADER_PATH).exists() {
        return Err(ReleaseError::SystemdBootEfiPathNotFound);
    }

    let partitions = fs::read_to_string("/proc/mounts").map_err(ReleaseError::ReadingPartitions)?;

    if partitions.contains("/recovery") {
        Ok(())
    } else {
        Err(ReleaseError::RecoveryNotFound)
    }
}
