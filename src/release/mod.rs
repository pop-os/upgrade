mod errors;
mod snapd;

use apt_fetcher::{
    apt_uris::{apt_uris, AptUri},
    SourcesLists, UpgradeRequest, Upgrader,
};
use envfile::EnvFile;
use futures::{stream, Future, Stream};
use std::{
    collections::HashSet,
    fs::{self, File},
    os::unix::fs::symlink,
    path::Path,
    sync::Arc,
};
use systemd_boot_conf::SystemdBootConf;

use crate::{
    daemon::DaemonRuntime,
    release_api::{ApiError, Release},
    repair,
};
use apt_cli_wrappers::*;
use ubuntu_version::{Codename, Version, VersionError};

pub use self::errors::{RelResult, ReleaseError};

const REQUIRED_PPAS: &[&str] = &[
    "archive.ubuntu.com/ubuntu",
    "ppa.launchpad.net/system76/pop/ubuntu",
    "apt.pop-os.org/proprietary",
];

const CORE_PACKAGES: &[&str] = &["linux-generic", "pop-desktop"];
const DPKG_LOCK: &str = "/var/lib/dpkg/lock";
const LISTS_LOCK: &str = "/var/lib/apt/lists/lock";
const RELEASE_FETCH_FILE: &str = "/pop_preparing_release_upgrade";
const STARTUP_UPGRADE_FILE: &str = "/pop-upgrade";
const SYSTEM_UPDATE: &str = "/system-update";
const SYSTEMD_BOOT_LOADER_PATH: &str = "/boot/efi/loader";
const SYSTEMD_BOOT_LOADER: &str = "/boot/efi/EFI/systemd/systemd-bootx64.efi";

pub fn is_required_ppa(ppa: &str) -> bool {
    REQUIRED_PPAS.into_iter().any(|&required| ppa.contains(required))
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

pub enum BuildStatus {
    ConnectionIssue(reqwest::Error),
    ServerStatus(reqwest::Error),
    InternalIssue(ApiError),
    Build(u16),
}

impl BuildStatus {
    pub fn is_ok(&self) -> bool {
        if let BuildStatus::Build(_) = *self {
            true
        } else {
            false
        }
    }

    pub fn status_code(&self) -> i16 {
        match *self {
            BuildStatus::ConnectionIssue(_) => -3,
            BuildStatus::ServerStatus(_) => -2,
            BuildStatus::InternalIssue(_) => -1,
            BuildStatus::Build(build) => build as i16,
        }
    }
}

impl From<Result<u16, ApiError>> for BuildStatus {
    fn from(result: Result<u16, ApiError>) -> Self {
        match result {
            Err(ApiError::Get(why)) => BuildStatus::ConnectionIssue(why),
            Err(ApiError::Status(why)) => BuildStatus::ServerStatus(why),
            Err(otherwise) => BuildStatus::InternalIssue(otherwise),
            Ok(build) => BuildStatus::Build(build),
        }
    }
}

pub struct ReleaseStatus {
    pub current: Box<str>,
    pub next:    Box<str>,
    pub build:   BuildStatus,
    is_lts:      bool,
}

impl ReleaseStatus {
    pub fn is_lts(&self) -> bool { self.is_lts }
}

pub fn check(development: bool) -> Result<ReleaseStatus, VersionError> {
    find_next_release(development, Version::detect, Release::build_exists)
}

pub fn check_current(version: Option<&str>) -> Option<(String, u16)> {
    find_current_release(Version::detect, Release::build_exists, version)
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
}

impl From<UpgradeEvent> for &'static str {
    fn from(action: UpgradeEvent) -> Self {
        match action {
            UpgradeEvent::UpdatingPackageLists => "updating package lists for the current release",
            UpgradeEvent::FetchingPackages => "fetching updated packages for the current release",
            UpgradeEvent::UpgradingPackages => "upgrading packages for the current release",
            UpgradeEvent::InstallingPackages => {
                "ensuring that system-critical packages are installed"
            }
            UpgradeEvent::UpdatingSourceLists => "updating the source lists to the new release",
            UpgradeEvent::FetchingPackagesForNewRelease => "fetching packages for the new release",
            UpgradeEvent::AttemptingLiveUpgrade => "attempting live upgrade to the new release",
            UpgradeEvent::AttemptingSystemdUnit => {
                "setting up the system to perform an offline upgrade on the next boot"
            }
            UpgradeEvent::AttemptingRecovery => {
                "setting up the recovery partition to install the new release"
            }
            UpgradeEvent::Success => "new release is ready to install",
            UpgradeEvent::SuccessLive => "new release was successfully installed",
            UpgradeEvent::Failure => "an error occurred while setting up the release upgrade",
            UpgradeEvent::AptFilesLocked => "waiting on a process holding the apt lock files",
        }
    }
}

impl<'a> DaemonRuntime<'a> {
    /// Get a list of APT URIs to fetch for this operation, and then fetch them.
    pub fn apt_fetch(
        &mut self,
        uris: Vec<AptUri>,
        func: Arc<dyn Fn(FetchEvent) + Send + Sync>,
    ) -> RelResult<()> {
        (*func)(FetchEvent::Init(uris.len()));
        let _lock_files = hold_apt_locks()?;
        let func2 = func.clone();

        let client = self.client.clone();
        let stream_of_downloads = stream::iter_ok(uris);
        let buffered_stream = stream_of_downloads
            .map(move |uri| {
                func(FetchEvent::Fetching(uri.clone()));
                uri.fetch(&client)
            })
            .buffer_unordered(8)
            .for_each(move |uri| {
                func2(FetchEvent::Fetched(uri.clone()));
                Ok(())
            })
            .map_err(|(uri, why)| ReleaseError::PackageFetch(uri.name, uri.uri, why));

        self.runtime.block_on(buffered_stream).map(|_| ())
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
            .map(Codename::from)
            .map(<&'static str>::from)
            .unwrap_or(current);

        let new =
            new.parse::<Version>().map(Codename::from).map(<&'static str>::from).unwrap_or(new);

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

        // Update the package lists for the current release.
        apt_update(|ready| lock_or(ready, UpgradeEvent::UpdatingPackageLists))
            .map_err(ReleaseError::CurrentUpdate)?;

        // Fetch required packages for upgrading the current release.
        (*logger)(UpgradeEvent::FetchingPackages);
        let mut uris = apt_uris(&["full-upgrade"]).map_err(ReleaseError::AptList)?;

        // Also include the packages which we must have installed.
        uris.extend_from_slice(
            &apt_uris(&{
                let mut args = vec!["install"];
                args.extend_from_slice(CORE_PACKAGES);
                args
            })
            .map_err(ReleaseError::AptList)?,
        );

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
        let mut upgrader = self.fetch_new_release_packages(logger, retain, fetch, from, to)?;
        let result = self.perform_action(logger, action, from, to);

        // We know that an offline install will trigger the upgrade script at init.
        // We want to ensure that the recovery partition has removed this file on completion.
        if let UpgradeMethod::Offline = action {
            let _ = fs::remove_file(RELEASE_FETCH_FILE);
        }

        if let Err(ref why) = result {
            (*logger)(UpgradeEvent::Failure);
            rollback(&mut upgrader, why);
        } else {
            (*logger)(UpgradeEvent::Success);
        }

        result
    }

    fn perform_action(
        &mut self,
        logger: &dyn Fn(UpgradeEvent),
        action: UpgradeMethod,
        from: &str,
        to: &str,
    ) -> RelResult<()> {
        match action {
            UpgradeMethod::Offline => {
                (*logger)(UpgradeEvent::AttemptingSystemdUnit);
                Self::systemd_upgrade_set(from, to)
            }
            UpgradeMethod::Recovery => {
                (*logger)(UpgradeEvent::AttemptingRecovery);
                set_recovery_as_default_boot_option("UPGRADE").map(|_| ())
            }
        }
    }

    /// Validate that the pre-required files for performing a system upgrade are in place.
    fn systemd_upgrade_prereq_check() -> RelResult<()> {
        const REQUIRED_UPGRADE_FILES: [&str; 3] = [
            "/usr/lib/pop-upgrade/upgrade.sh",
            "/lib/systemd/system/pop-upgrade-init.service",
            "/lib/systemd/system/system-update.target.wants/pop-upgrade-init.service",
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

    /// Create the system upgrade files that systemd will check for at startup.
    fn systemd_upgrade_set(from: &str, to: &str) -> RelResult<()> {
        let current =
            from.parse::<Version>().map(Codename::from).map(<&'static str>::from).unwrap_or(from);

        let new = to.parse::<Version>().map(Codename::from).map(<&'static str>::from).unwrap_or(to);

        fs::write(STARTUP_UPGRADE_FILE, &format!("{} {}", current, new))
            .and_then(|_| symlink("/var/cache/apt/archives", SYSTEM_UPDATE))
            .map_err(ReleaseError::StartupFileCreation)
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
        let mut upgrader = self.release_upgrade(retain, &current, &next)?;

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

        match self.attempt_fetch(logger, fetch) {
            Ok(_) => info!("packages fetched successfully"),
            Err(why) => {
                rollback(&mut upgrader, &why);

                return Err(why);
            }
        }

        Ok(upgrader)
    }
}

fn rollback<E: ::std::fmt::Display>(upgrader: &mut Upgrader, why: &E) {
    error!("failed to fetch packages: {}", why);
    warn!("attempting to roll back apt release files");
    if let Err(why) = upgrader.revert_apt_sources() {
        error!("failed to revert release name changes to source lists in /etc/apt/: {}", why);
    }
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
                LoaderEntry::Current => e.filename.to_lowercase().ends_with("current"),
                LoaderEntry::Recovery => e.filename.to_lowercase().starts_with("recovery"),
            })
            .ok_or(ReleaseError::MissingRecoveryEntry)?;

        loader_conf.default = Some(recovery_entry.filename.to_owned());
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

fn format_version(version: Version) -> String { format!("{}.{:02}", version.major, version.minor) }

fn find_current_release(
    version_detect: fn() -> Result<Version, VersionError>,
    release_exists: fn(&str, &str) -> Result<u16, ApiError>,
    version: Option<&str>,
) -> Option<(String, u16)> {
    if let Some(version) = version {
        let build = release_exists(version, "intel").ok()?;
        return Some((version.into(), build));
    }

    let mut current = version_detect().ok()?;
    let mut current_str = format_version(current);
    let mut available = release_exists(&current_str, "intel").ok()?;

    let mut next = current.next_release();
    let mut next_str = format_version(next);

    while let Ok(build) = release_exists(&next_str, "intel") {
        available = build;
        current = next;
        current_str = next_str;
        next = current.next_release();
        next_str = format_version(next);
    }

    Some((current_str, available))
}

fn find_next_release(
    development: bool,
    version_detect: fn() -> Result<Version, VersionError>,
    release_exists: fn(&str, &str) -> Result<u16, ApiError>,
) -> Result<ReleaseStatus, VersionError> {
    let current = version_detect()?;
    let mut next = current.next_release();
    let mut next_str = format_version(next);
    let mut available = release_exists(&next_str, "intel");

    if available.is_ok() {
        let mut next_next = next.next_release();
        let mut next_next_str = format_version(next_next);

        let mut last_build_status = release_exists(&next_next_str, "intel");

        loop {
            if let Ok(build) = last_build_status {
                available = Ok(build);
                next = next_next;
                next_str = next_next_str;
                next_next = next.next_release();
                next_next_str = format_version(next_next);
            } else if development {
                // If the next release is available, then the development
                // release is the release after the last available release.
                next = next.next_release();
                next_str = format_version(next);
                available = last_build_status;

                break;
            } else {
                break;
            }

            last_build_status = release_exists(&next_next_str, "intel");
        }
    }

    Ok(ReleaseStatus {
        current: format_version(current).into(),
        next:    next_str.into(),
        build:   available.into(),
        is_lts:  current.is_lts(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ubuntu_version::{Version, VersionError};

    fn v1804() -> Result<Version, VersionError> { Ok(Version { major: 18, minor: 4, patch: 0 }) }

    fn v1810() -> Result<Version, VersionError> { Ok(Version { major: 18, minor: 10, patch: 0 }) }

    fn v1904() -> Result<Version, VersionError> { Ok(Version { major: 19, minor: 4, patch: 0 }) }

    fn releases_up_to_1904(release: &str, _kind: &str) -> Result<u16, ApiError> {
        match release {
            "18.04" | "18.10" | "19.04" => Ok(1),
            _ => Err(ApiError::BuildNaN("".into())),
        }
    }

    fn releases_up_to_1910(release: &str, kind: &str) -> Result<u16, ApiError> {
        releases_up_to_1904(release, kind).or_else(|_| {
            if release == "19.10" {
                Ok(1)
            } else {
                Err(ApiError::BuildNaN("".into()))
            }
        })
    }

    #[test]
    fn release_check() {
        let mut status = find_next_release(false, v1804, releases_up_to_1910).unwrap();
        assert!("19.10" == dbg!(status.next.as_ref()) && status.build.is_ok());

        status = find_next_release(false, v1810, releases_up_to_1910).unwrap();
        assert!("19.10" == dbg!(status.next.as_ref()) && status.build.is_ok());

        status = find_next_release(false, v1810, releases_up_to_1904).unwrap();
        assert!("19.04" == dbg!(status.next.as_ref()) && status.build.is_ok());

        status = find_next_release(false, v1904, releases_up_to_1904).unwrap();
        assert!("19.10" == dbg!(status.next.as_ref()) && !status.build.is_ok());

        status = find_next_release(true, v1804, releases_up_to_1910).unwrap();
        assert!("20.04" == dbg!(status.next.as_ref()) && !status.build.is_ok());
    }

    #[test]
    fn current_release_check() {
        let (current, _build) = find_current_release(v1804, releases_up_to_1910, None).unwrap();
        assert!("19.10" == current.as_str());

        let (current, _build) = find_current_release(v1904, releases_up_to_1904, None).unwrap();
        assert!("19.04" == current.as_str());

        let (current, _build) =
            find_current_release(v1904, releases_up_to_1904, Some("18.04")).unwrap();
        assert!("18.04" == current.as_str());
    }
}
