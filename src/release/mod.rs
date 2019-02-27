mod errors;

use apt_fetcher::apt_uris::{apt_uris, AptUri};
use apt_fetcher::{SourcesList, UpgradeRequest, Upgrader};
use envfile::EnvFile;
use futures::{stream, Future, Stream};
use std::fs::{self, File};
use std::os::unix::fs::symlink;
use std::path::Path;
use std::sync::Arc;
use systemd_boot_conf::SystemdBootConf;

use crate::apt_wrappers::*;
use crate::daemon::DaemonRuntime;
use crate::release_api::Release;
use crate::repair;
use ubuntu_version::{Codename, Version};

pub use self::errors::{RelResult, ReleaseError};

const CORE_PACKAGES: &[&str] = &["pop-desktop"];
const RELEASE_FETCH_FILE: &str = "/release_fetch_in_progress";
const SYSTEMD_BOOT_LOADER: &str = "/boot/efi/EFI/systemd/systemd-bootx64.efi";
const SYSTEMD_BOOT_LOADER_PATH: &str = "/boot/efi/loader";

pub fn check() -> RelResult<(String, String, bool)> {
    let current = Version::detect()?;
    let next = format!("{}", current.next_release());
    let current = format!("{}", current);
    let available = Release::get_release(&next, "intel").is_ok();
    Ok((current, next, available))
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
}

impl From<UpgradeEvent> for &'static str {
    fn from(action: UpgradeEvent) -> Self {
        match action {
            UpgradeEvent::UpdatingPackageLists => "updating package lists for the current release",
            UpgradeEvent::FetchingPackages => "fetching updated packages for the current release",
            UpgradeEvent::UpgradingPackages => "upgrading packages for the current release",
            UpgradeEvent::InstallingPackages => {
                "ensuring that system-critical packages are isntalled"
            }
            UpgradeEvent::UpdatingSourceLists => "updating the source lists to the new release",
            UpgradeEvent::FetchingPackagesForNewRelease => "fetching packages for the new release",
            UpgradeEvent::AttemptingLiveUpgrade => "attempting live upgrade to the new release",
            UpgradeEvent::AttemptingSystemdUnit => {
                "creating a systemd unit for installing the new release"
            }
            UpgradeEvent::AttemptingRecovery => {
                "setting up the recovery partition to install the new release"
            }
            UpgradeEvent::Success => "new release is ready to install",
            UpgradeEvent::SuccessLive => "new release was successfully installed",
            UpgradeEvent::Failure => "an error occurred while setting up the upgrade",
        }
    }
}

impl<'a> DaemonRuntime<'a> {
    /// Get a list of APT URIs to fetch for this operation, and then fetch them.
    pub fn apt_fetch(
        &mut self,
        uris: Vec<AptUri>,
        func: Arc<Fn(FetchEvent) + Send + Sync>,
    ) -> RelResult<()> {
        (*func)(FetchEvent::Init(uris.len()));
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
            .map_err(ReleaseError::PackageFetch);

        self.runtime.block_on(buffered_stream).map(|_| ())
    }

    /// Check if release files can be upgraded, and then overwrite them with the new release.
    ///
    /// On failure, the original release files will be restored.
    pub fn release_upgrade(&mut self, current: &str, new: &str) -> Result<Upgrader, ReleaseError> {
        let current = current
            .parse::<Version>()
            .map(|c| <&'static str>::from(Codename::from(c)))
            .unwrap_or(current);

        let new =
            new.parse::<Version>().map(|c| <&'static str>::from(Codename::from(c))).unwrap_or(new);

        let sources = SourcesList::scan().unwrap();

        info!("checking if release can be upgraded from {} to {}", current, new);
        let mut upgrade = UpgradeRequest::new(self.client.clone(), sources, self.runtime)
            .send(current, new)
            .map_err(ReleaseError::Check)?;

        // In case the system abruptly shuts down after this point, create a file to signal
        // that packages were being fetched for a new release.
        fs::write(RELEASE_FETCH_FILE, &format!("{} {}", current, new))
            .map_err(ReleaseError::ReleaseFetchFile)?;

        info!("upgrade is possible -- updating release files");
        upgrade.overwrite_apt_sources().map_err(ReleaseError::Overwrite)?;

        Ok(upgrade)
    }

    /// Performs a live release upgrade via the daemon, with a callback for tracking progress.
    pub fn package_upgrade<C: Fn(AptUpgradeEvent)>(&mut self, mut callback: C) -> RelResult<()> {
        let callback = &mut callback;
        // If the first upgrade attempt fails, try to dpkg --configure -a and try again.
        if apt_upgrade(callback).is_err() {
            dpkg_configure_all().map_err(ReleaseError::DpkgConfigure)?;
            apt_upgrade(callback).map_err(ReleaseError::Upgrade)?;
        }

        Ok(())
    }

    /// Perform the release upgrade by updating release files, fetching packages required for the new
    /// release, and then setting the recovery partition as the default boot entry.
    pub fn upgrade(
        &mut self,
        action: UpgradeMethod,
        from: &str,
        to: &str,
        logger: &dyn Fn(UpgradeEvent),
        fetch: Arc<Fn(FetchEvent) + Send + Sync>,
    ) -> RelResult<()> {
        // Must be root for this operation.
        check_root()?;

        // Inhibit suspension and shutdown

        // Check the system and perform any repairs necessary for success.
        repair::repair().map_err(ReleaseError::Repair)?;

        // Ensure that prerequest files and mounts are available.
        match action {
            UpgradeMethod::Recovery => Self::recovery_upgrade_prereq_check()?,
            UpgradeMethod::Offline => Self::systemd_upgrade_prereq_check()?,
        }

        // Update the package lists for the current release.
        (*logger)(UpgradeEvent::UpdatingPackageLists);
        apt_update().map_err(ReleaseError::CurrentUpdate)?;

        // Fetch required packages for upgrading the current release.
        (*logger)(UpgradeEvent::FetchingPackages);
        let mut uris = apt_uris(&["full-upgrade"]).map_err(ReleaseError::AptList)?;

        // Also include the packages which we must have installed.
        let uris2 = apt_uris(&{
            let mut args = vec!["install"];
            args.extend_from_slice(CORE_PACKAGES);
            args
        })
        .map_err(ReleaseError::AptList)?;

        let nupdates = uris.len();
        let nfetched = uris.len();
        uris.extend_from_slice(&uris2);

        self.apt_fetch(uris, fetch.clone())?;

        if nupdates != 0 {
            // Upgrade the current release to the latest packages.
            (*logger)(UpgradeEvent::UpgradingPackages);
            apt_upgrade(&mut |_| {}).map_err(ReleaseError::Upgrade)?;
        }

        if nfetched != 0 {
            // Install any packages that are deemed critical.
            (*logger)(UpgradeEvent::InstallingPackages);
            apt_install(CORE_PACKAGES).map_err(ReleaseError::InstallCore)?;
        }

        // Update the source lists to the new release,
        // then fetch the packages required for the upgrade.
        let mut upgrader = self.fetch_new_release_packages(logger, fetch, from, to)?;
        let result = self.perform_action(logger, action);

        // Removing this will signal that we
        let _ = fs::remove_file(RELEASE_FETCH_FILE);

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
    ) -> RelResult<()> {
        match action {
            UpgradeMethod::Offline => {
                (*logger)(UpgradeEvent::AttemptingSystemdUnit);
                Self::systemd_upgrade_set()
            }
            UpgradeMethod::Recovery => {
                (*logger)(UpgradeEvent::AttemptingRecovery);
                set_recovery_as_default_boot_option()
            }
        }
    }

    fn recovery_upgrade_prereq_check() -> RelResult<()> {
        if !Path::new(SYSTEMD_BOOT_LOADER).exists() {
            return Err(ReleaseError::SystemdBootLoaderNotFound);
        }

        if !Path::new(SYSTEMD_BOOT_LOADER_PATH).exists() {
            return Err(ReleaseError::SystemdBootEfiPathNotFound);
        }

        if !Path::new("/recovery").exists() {
            return Err(ReleaseError::RecoveryNotFound);
        }

        Ok(())
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
    fn systemd_upgrade_set() -> RelResult<()> {
        const STARTUP_UPGRADE_FILE: &str = "/pop-upgrade";
        File::create(STARTUP_UPGRADE_FILE)
            .and_then(|_| symlink("/var/cache/apt/archives", "/system-update"))
            .map_err(ReleaseError::StartupFileCreation)
    }

    fn attempt_fetch(&mut self, fetch: Arc<Fn(FetchEvent) + Send + Sync>) -> RelResult<()> {
        info!("updated the package lists for the new relaese");
        apt_update().map_err(ReleaseError::ReleaseUpdate)?;

        info!("fetching packages for the new release");
        let uris = apt_uris(&["full-upgrade"]).map_err(ReleaseError::AptList)?;
        self.apt_fetch(uris, fetch)?;

        Ok(())
    }

    /// Update the release files and fetch packages for the new release.
    ///
    /// On failure, the original release files will be restored.
    fn fetch_new_release_packages(
        &mut self,
        logger: &dyn Fn(UpgradeEvent),
        fetch: Arc<Fn(FetchEvent) + Send + Sync>,
        current: &str,
        next: &str,
    ) -> RelResult<Upgrader> {
        (*logger)(UpgradeEvent::UpdatingSourceLists);
        let mut upgrader = self.release_upgrade(&current, &next)?;

        (*logger)(UpgradeEvent::FetchingPackagesForNewRelease);
        match self.attempt_fetch(fetch) {
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

/// Fetch the systemd-boot configuration, and designate the recovery partition as the default
/// boot option.
///
/// It will be up to the recovery partition to revert this change once it has completed its job.
fn set_recovery_as_default_boot_option() -> RelResult<()> {
    info!("gathering systemd-boot configuration information");

    let mut systemd_boot_conf =
        SystemdBootConf::new("/boot/efi").map_err(ReleaseError::SystemdBootConf)?;

    {
        info!("found the systemd-boot config -- searching for the recovery partition");
        let SystemdBootConf { ref entries, ref mut loader_conf, .. } = systemd_boot_conf;
        let recovery_entry = entries
            .iter()
            .find(|e| e.title == "Pop!_OS Recovery")
            .ok_or(ReleaseError::MissingRecoveryEntry)?;

        loader_conf.default = Some(recovery_entry.filename.to_owned());
    }

    info!("found the recovery partition -- setting it as the default boot entry");
    systemd_boot_conf.overwrite_loader_conf().map_err(ReleaseError::SystemdBootConfOverwrite)?;

    EnvFile::new(Path::new("/recovery/recovery.conf"))
        .map_err(ReleaseError::RecoveryConfOpen)?
        .update("UPGRADE", "1")
        .write()
        .map_err(ReleaseError::RecoveryUpdate)
}

pub enum FetchEvent {
    Fetching(AptUri),
    Fetched(AptUri),
    Init(usize),
}

fn check_root() -> RelResult<()> {
    if unsafe { libc::geteuid() } != 0 {
        Err(ReleaseError::NotRoot)
    } else {
        Ok(())
    }
}

pub fn release_fetch_cleanup() {
    if let Ok(data) = fs::read_to_string(RELEASE_FETCH_FILE) {
        let mut iter = data.split(' ');
        if let (Some(current), Some(next)) = (iter.next(), iter.next()) {
            if let Ok(mut lists) = SourcesList::scan() {
                lists.dist_replace(next, current);
                let _ = lists.write_sync();
            }
        }

        let _ = fs::remove_file(RELEASE_FETCH_FILE);
        let _ = apt_update();
    }
}
