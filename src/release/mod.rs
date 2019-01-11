mod errors;

use apt_fetcher::{UpgradeRequest, Upgrader, SourcesList};
use apt_fetcher::apt_uris::apt_uris;
use atty::{self, Stream as AttyStream};
use clap::ArgMatches;
use futures::{stream, Future, Stream};
use promptly::prompt;
use reqwest::r#async::Client;
use std::io;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use systemd_boot_conf::SystemdBootConf;
use tokio::runtime::Runtime;

use ::release_version::{detect_version};
use ::release_api::Release;
use ::ubuntu_codename::UbuntuCodename;
use ::status::StatusExt;

pub use self::errors::{ReleaseError, RelResult};

const CORE_PACKAGES: &[&str] = &["pop-desktop"];

pub fn release(matches: &ArgMatches) -> RelResult<()> {
    match matches.subcommand() {
        ("check", _) => check_release()?,
        ("upgrade", Some(matches)) => do_upgrade(matches)?,
        _ => unimplemented!()
    }

    Ok(())
}

fn check_release() -> RelResult<()> {
    let (current, next) = detect_version()?;

    println!(
        "      Current Release: {}\n         Next Release: {}\nNew Release Available: {}",
        current,
        next,
        Release::get_release(&next, "intel").is_ok()
    );

    Ok(())
}

/// Perform the release upgrade by updating release files, fetching packages required for the new
/// release, and then setting the recovery partition as the default boot entry.
fn do_upgrade(matches: &ArgMatches) -> RelResult<()> {
    // Must be root for this operation.
    check_root()?;

    // Create the tokio runtime to share between requests.
    let runtime = &mut Runtime::new().expect("failed to initialize tokio runtime");

    // This client contains a thread pool for performing HTTP/s requests.
    let client = Arc::new(
        Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .expect("failed to initialize reqwest client")
    );

    // Update the package lists for the current release.
    info!("updating package lists for the current release");
    apt_update().map_err(ReleaseError::CurrentUpdate)?;

    // Fetch required packages for upgrading the current release.
    info!("fetching updated packages for the current release");
    let nupdates = apt_fetch(runtime, client.clone(), &["full-upgrade"])?;

    // Also include the packages which we must have installed.
    let nfetched = apt_fetch(runtime, client.clone(), &{
        let mut args = vec!["install"];
        args.extend_from_slice(CORE_PACKAGES);
        args
    })?;

    if nupdates != 0 {
        // Upgrade the current release to the latest packages.
        info!("upgrading packages for the current release");
        apt_upgrade().map_err(ReleaseError::Upgrade)?;
    } else {
        info!("no packages require upgrading -- ready to proceed");
    }

    if nfetched != 0 {
        // Install any packages that are deemed critical.
        info!("ensuring that system-critical packages are installed");
        apt_install(CORE_PACKAGES).map_err(ReleaseError::InstallCore)?;
    } else {
        info!("system-critical packages are installed -- ready to proceed");
    }

    // Update the source lists to the new release,
    // then fetch the packages required for the upgrade.
    let _upgrader = fetch_new_release_packages(runtime, client)?;

    const SYSTEMD_BOOT_LOADER: &str = "/boot/efi/EFI/systemd/systemd-bootx64.efi";
    const SYSTEMD_BOOT_LOADER_PATH: &str = "/boot/efi/loader";

    enum Action {
        Exit,
        LiveUpgrade
    }

    let action = if matches.is_present("live") {
        Action::LiveUpgrade
    } else if Path::new(SYSTEMD_BOOT_LOADER).exists() && Path::new(SYSTEMD_BOOT_LOADER_PATH).exists() {
        info!("found the systemd-boot loader and loader configuration directory");
        match set_recovery_as_default_boot_option() {
            Ok(()) => Action::Exit,
            Err(ReleaseError::MissingRecoveryEntry) => {
                warn!("an entry for the recovery partition was not found -- \
                    asking to install upgrades without it");

                Action::LiveUpgrade
            }
            Err(why) => return Err(why)
        }
    } else {
        warn!("system is not configured with systemd-boot -- \
            asking to install upgrades without it");

        Action::LiveUpgrade
    };

    if let Action::LiveUpgrade = action {
        let upgrade = atty::isnt(AttyStream::Stdout)
            || prompt::<bool, &str>("Attempt live system upgrade? Ensure that you have backups");

        if upgrade {
            info!("attempting release upgrade");
            apt_upgrade().map_err(ReleaseError::ReleaseUpgrade)?;
        }
    }

    Ok(())
}

/// Fetch the systemd-boot configuration, and designate the recovery partition as the default
/// boot option.
///
/// It will be up to the recovery partition to revert this change once it has completed its job.
fn set_recovery_as_default_boot_option() -> RelResult<()> {
    info!("gathering systemd-boot configuration information");
    let mut systemd_boot_conf = SystemdBootConf::new("/boot/efi")
        .map_err(ReleaseError::SystemdBootConf)?;

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
    systemd_boot_conf.overwrite_loader_conf()
        .map_err(ReleaseError::SystemdBootConfOverwrite)
}

/// Update the release files and fetch packages for the new release.
///
/// On failure, the original release files will be restored.
fn fetch_new_release_packages(runtime: &mut Runtime, client: Arc<Client>) -> RelResult<Upgrader> {
    let (current, next) = detect_version()?;
    info!("attempting to upgrade to the new release");
    let mut upgrader = release_upgrade(runtime, client.clone(), &current, &next)?;

    fn attempt_fetch(runtime: &mut Runtime, client: Arc<Client>) -> RelResult<()> {
        info!("updated the package lists for the new relaese");
        apt_update().map_err(ReleaseError::ReleaseUpdate)?;

        info!("fetching packages for the new release");
        apt_fetch(runtime, client, &["full-upgrade"])?;

        Ok(())
    }

    match attempt_fetch(runtime, client) {
        Ok(_) => info!("packages fetched successfully"),
        Err(why) => {
            error!("failed to fetch packages: {}", why);
            warn!("rolling back apt release files");
            if let Err(why) = upgrader.revert_apt_sources() {
                error!("failed to revert release name changes to source lists in /etc/apt/: {}", why);
            }

            ::std::process::exit(1);
        }
    }

    Ok(upgrader)
}

/// Check if release files can be upgraded, and then overwrite them with the new release.
///
/// On failure, the original release files will be restored.
fn release_upgrade(runtime: &mut Runtime, client: Arc<Client>, current: &str, new: &str) -> Result<Upgrader, ReleaseError> {
    let current = UbuntuCodename::from_version(current).map_or(current, |c| c.into_codename());
    let new = UbuntuCodename::from_version(new).map_or(new, |c| c.into_codename());

    let sources = SourcesList::scan().unwrap();

    info!("checking if release can be upgraded from {} to {}", current, new);
    let mut upgrade = UpgradeRequest::new(client, sources, runtime)
        .send(current, new)
        .map_err(ReleaseError::Check)?;

    info!("upgrade is possible -- updating release files");
    upgrade.overwrite_apt_sources()
        .map_err(ReleaseError::Overwrite)?;

    Ok(upgrade)
}

/// Get a list of APT URIs to fetch for this operation, and then fetch them.
fn apt_fetch(runtime: &mut Runtime, client: Arc<Client>, args: &[&str]) -> RelResult<usize> {
    let apt_uris = apt_uris(args).map_err(ReleaseError::AptList)?;
    let size: u64 = apt_uris.iter().map(|v| v.size).sum();
    let npackages = apt_uris.len();

    info!("fetching {} packages ({} MiB total)", npackages, size / 1024 / 1024);
    let stream_of_downloads = stream::iter_ok(apt_uris);
    let buffered_stream = stream_of_downloads
        .map(move |uri| uri.fetch(&client))
        .buffer_unordered(8)
        .for_each(|_| Ok(()))
        .map_err(ReleaseError::PackageFetch);

    runtime.block_on(buffered_stream).map(|_| npackages)
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
fn apt_upgrade() -> io::Result<()> {
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
