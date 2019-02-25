mod errors;

use apt_fetcher::apt_uris::{apt_uris, AptUri};
use apt_fetcher::{SourcesList, UpgradeRequest, Upgrader};
use envfile::EnvFile;
use futures::{stream, Future, Stream};
use std::fs::File;
use std::io;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use systemd_boot_conf::SystemdBootConf;

use crate::daemon::DaemonRuntime;
use crate::release_api::Release;
use crate::repair;
use crate::status::StatusExt;
use crate::ubuntu_version::{Codename, Version};

pub use self::errors::{RelResult, ReleaseError};

const CORE_PACKAGES: &[&str] = &["pop-desktop"];
const SYSTEMD_BOOT_LOADER: &str = "/boot/efi/EFI/systemd/systemd-bootx64.efi";
const SYSTEMD_BOOT_LOADER_PATH: &str = "/boot/efi/loader";

pub fn check() -> RelResult<(String, String, bool)> {
    let current = Version::detect()?;
    let next = format!("{}", current.next());
    let current = format!("{}", current);
    let available = Release::get_release(&next, "intel").is_ok();
    Ok((current, next, available))
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, FromPrimitive, PartialEq)]
pub enum UpgradeMethod {
    Live = 1,
    Systemd = 2,
    Recovery = 3,
}

impl From<UpgradeMethod> for &'static str {
    fn from(action: UpgradeMethod) -> Self {
        match action {
            UpgradeMethod::Live => "live, in-place upgrade",
            UpgradeMethod::Systemd => "systemd oneshot",
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

        info!("upgrade is possible -- updating release files");
        upgrade.overwrite_apt_sources().map_err(ReleaseError::Overwrite)?;

        Ok(upgrade)
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

        // Check the system and perform any repairs necessary for success.
        repair::repair().map_err(ReleaseError::Repair)?;

        // Update the package lists for the current release.
        (*logger)(UpgradeEvent::UpdatingPackageLists);
        apt_update().map_err(ReleaseError::CurrentUpdate)?;

        // Fetch required packages for upgrading the current release.
        (*logger)(UpgradeEvent::FetchingPackages);
        let uris = apt_uris(&["full-upgrade"]).map_err(ReleaseError::AptList)?;
        let nupdates = uris.len();

        // Also include the packages which we must have installed.
        let uris = apt_uris(&{
            let mut args = vec!["install"];
            args.extend_from_slice(CORE_PACKAGES);
            args
        })
        .map_err(ReleaseError::AptList)?;
        let nfetched = uris.len();
        self.apt_fetch(uris, fetch.clone())?;

        if nupdates != 0 {
            // Upgrade the current release to the latest packages.
            (*logger)(UpgradeEvent::UpgradingPackages);
            apt_upgrade().map_err(ReleaseError::Upgrade)?;
        }

        if nfetched != 0 {
            // Install any packages that are deemed critical.
            (*logger)(UpgradeEvent::InstallingPackages);
            apt_install(CORE_PACKAGES).map_err(ReleaseError::InstallCore)?;
        }

        // Update the source lists to the new release,
        // then fetch the packages required for the upgrade.
        let _upgrader = self.fetch_new_release_packages(logger, fetch, from, to)?;

        match action {
            UpgradeMethod::Live => {
                (*logger)(UpgradeEvent::AttemptingLiveUpgrade);
                apt_upgrade().map_err(ReleaseError::ReleaseUpgrade)?;

                (*logger)(UpgradeEvent::SuccessLive);
                return Ok(());
            }
            UpgradeMethod::Systemd => {
                (*logger)(UpgradeEvent::AttemptingSystemdUnit);
                Self::systemd_upgrade_prereq_check()?;
                Self::systemd_upgrade_set()?;
            }
            UpgradeMethod::Recovery => {
                (*logger)(UpgradeEvent::AttemptingRecovery);
                set_recovery_as_default_boot_option()?;
            }
        }

        (*logger)(UpgradeEvent::Success);
        Ok(())
    }

    /// Validate that the pre-required files for performing a system upgrade are in place.
    fn systemd_upgrade_prereq_check() -> RelResult<()> {
        const REQUIRED_UPGRADE_FILES: [&str; 3] = [
            "/usr/lib/pop-upgrade/upgrade.sh",
            "/lib/systemd/system/pop-upgrade.service",
            "/lib/systemd/system/system-update.target.wants/pop-upgrade.service",
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
                error!("failed to fetch packages: {}", why);
                warn!("attempting to roll back apt release files");
                if let Err(why) = upgrader.revert_apt_sources() {
                    error!(
                        "failed to revert release name changes to source lists in /etc/apt/: {}",
                        why
                    );
                }

                return Err(why);
            }
        }

        Ok(upgrader)
    }
}

/// Fetch the systemd-boot configuration, and designate the recovery partition as the default
/// boot option.
///
/// It will be up to the recovery partition to revert this change once it has completed its job.
fn set_recovery_as_default_boot_option() -> RelResult<()> {
    info!("gathering systemd-boot configuration information");

    if !Path::new(SYSTEMD_BOOT_LOADER).exists() {
        return Err(ReleaseError::SystemdBootLoaderNotFound);
    }

    if !Path::new(SYSTEMD_BOOT_LOADER_PATH).exists() {
        return Err(ReleaseError::SystemdBootEfiPathNotFound);
    }

    if !Path::new("/recovery").exists() {
        return Err(ReleaseError::RecoveryNotFound);
    }

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
}

/// Execute the apt command non-interactively, using whichever additional arguments are provided.
fn apt_noninteractive<F: FnMut(&mut Command) -> &mut Command>(mut func: F) -> io::Result<()> {
    func(Command::new("apt-get").env("DEBIAN_FRONTEND", "noninteractive"))
        .status()
        .and_then(StatusExt::as_result)
}

/// apt-get update
fn apt_update() -> io::Result<()> {
    apt_noninteractive(|cmd| cmd.arg("update"))
}

/// apt-get upgrade
pub fn apt_upgrade() -> io::Result<()> {
    apt_noninteractive(|cmd| cmd.arg("full-upgrade"))
}

/// apt-get install
fn apt_install(packages: &[&str]) -> io::Result<()> {
    apt_noninteractive(move |cmd| cmd.arg("install").args(packages))
}

fn check_root() -> RelResult<()> {
    if unsafe { libc::geteuid() } != 0 {
        Err(ReleaseError::NotRoot)
    } else {
        Ok(())
    }
}
