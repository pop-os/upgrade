use apt_fetcher::{DistUpgradeError, UpgradeRequest, Upgrader, SourcesList};
use apt_fetcher::apt_uris::{apt_uris, AptUri, AptUriError};
use apt_keyring::AptKeyring;
use async_fetcher::FetchError;
use clap::ArgMatches;
use futures::{stream, Future, Stream};
use reqwest::r#async::Client;
use tokio::runtime::Runtime;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::io;
use std::process::Command;
use status::StatusExt;

use ::release_architecture::{detect_arch, ReleaseArchError};
use ::release_version::{detect_version, ReleaseVersionError};
use ::release_api::{ApiError, Release};

const CORE_PACKAGES: &[&str] = &["pop-desktop"];

pub type RelResult<T> = Result<T, ReleaseError>;

#[derive(Debug, Error)]
pub enum ReleaseError {
    #[error(display = "failed to fetch release architecture: {}", _0)]
    ReleaseArch(ReleaseArchError),
    #[error(display = "failed to fetch release versions: {}", _0)]
    ReleaseVersion(ReleaseVersionError),
    #[error(display = "failed to fetch apt URIs to fetch: {}", _0)]
    AptList(AptUriError),
    #[error(display = "unable to upgrade to next release: {}", _0)]
    Check(DistUpgradeError),
    #[error(display = "failure to overwrite release files: {}", _0)]
    Overwrite(DistUpgradeError),
    #[error(display = "root is required for this action: rerun with `sudo`")]
    NotRoot,
    #[error(display = "fetch of package failed: {}", _0)]
    PackageFetch(FetchError),
    #[error(display = "failed to update package lists for the current release: {}", _0)]
    CurrentUpdate(io::Error),
    #[error(display = "failed to update package lists for the new release: {}", _0)]
    ReleaseUpdate(io::Error),
    #[error(display = "failed to perform apt upgrade of the current release: {}", _0)]
    Upgrade(io::Error),
    #[error(display = "failed to install core packages: {}", _0)]
    InstallCore(io::Error)
}

impl From<ReleaseVersionError> for ReleaseError {
    fn from(why: ReleaseVersionError) -> Self {
        ReleaseError::ReleaseVersion(why)
    }
}

impl From<ReleaseArchError> for ReleaseError {
    fn from(why: ReleaseArchError) -> Self {
        ReleaseError::ReleaseArch(why)
    }
}

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

pub enum UbuntuCodename {
    Bionic,
    Cosmic,
    Disco
}

impl UbuntuCodename {
    pub fn from_version(version: &str) -> Option<Self> {
        let release = match version {
            "18.04" => UbuntuCodename::Bionic,
            "18.10" => UbuntuCodename::Cosmic,
            "19.04" => UbuntuCodename::Disco,
            _ => return None
        };

        Some(release)
    }

    pub fn as_codename(self) -> &'static str {
        match self {
            UbuntuCodename::Bionic => "bionic",
            UbuntuCodename::Cosmic => "cosmic",
            UbuntuCodename::Disco => "disco"
        }
    }

    pub fn as_version(self) -> &'static str {
        match self {
            UbuntuCodename::Bionic => "18.04",
            UbuntuCodename::Cosmic => "18.10",
            UbuntuCodename::Disco => "19.04"
        }
    }
}

/// Perform the release upgrade by updating release files, fetching packages required for the new
/// release, and then setting the recovery partition as the default boot entry.
fn do_upgrade(matches: &ArgMatches) -> RelResult<()> {
    // Must be root for this operation.
    check_root()?;

    // This client contains a thread pool for performing HTTP/s requests.
    let client = Arc::new(
        Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap()
    );

    // Update the package lists for the current release.
    apt_update().map_err(ReleaseError::CurrentUpdate)?;

    // Fetch required packages for upgrading the current release.
    apt_fetch(client.clone(), &["full-upgrade"])?;

    // Also include the packages which we must have installed.
    apt_fetch(client.clone(), &{
        let mut args = vec!["install"];
        args.extend_from_slice(CORE_PACKAGES);
        args
    })?;

    // Upgrade the current release to the latest packages.
    apt_upgrade().map_err(ReleaseError::Upgrade)?;

    // Install any packages that are deemed critical.
    apt_install(CORE_PACKAGES).map_err(ReleaseError::InstallCore)?;

    // Update the source lists to the new release,
    // then fetch the packages required for the upgrade.
    do_release_fetch(client)?;

    Ok(())
}

/// Update the release files and fetch packages for the new release.
///
/// On failure, the original release files will be restored.
fn do_release_fetch(client: Arc<Client>) -> RelResult<()> {
    let (current, next) = detect_version()?;
    let mut upgrader = release_upgrade(client.clone(), &current, &next)?;

    fn attempt_fetch(client: Arc<Client>) -> RelResult<()> {
        apt_update().map_err(ReleaseError::ReleaseUpdate)?;
        apt_fetch(client, &["full-upgrade"])
    }

    match attempt_fetch(client) {
        Ok(_) => println!("packages fetched successfully"),
        Err(why) => {
            eprintln!("failed to fetch packages: {}", why);
            eprintln!("rolling back apt release files");
            if let Err(why) = upgrader.revert_apt_sources() {
                eprintln!("failed to revert release name changes to source lists in /etc/apt/");
            }

            ::std::process::exit(1);
        }
    }

    Ok(())
}

/// Check if release files can be upgraded, and then overwrite them with the new release.
///
/// On failure, the original release files will be restored.
fn release_upgrade(client: Arc<Client>, current: &str, new: &str) -> Result<Upgrader, ReleaseError> {
    let current = UbuntuCodename::from_version(current).map_or(current, |c| c.as_codename());
    let new = UbuntuCodename::from_version(new).map_or(new, |c| c.as_codename());

    let sources = SourcesList::scan().unwrap();
    let client = Arc::new(Client::new());

    println!("Checking if release can be upgraded from {} to {}", current, new);
    let mut upgrade = UpgradeRequest::new(client, sources)
        .send(current, new)
        .map_err(ReleaseError::Check)?;

    println!("Upgrade is possible -- updating release files");
    upgrade.overwrite_apt_sources()
        .map_err(ReleaseError::Overwrite)?;

    Ok(upgrade)
}

/// Get a list of APT URIs to fetch for this operation, and then fetch them.
fn apt_fetch(client: Arc<Client>, args: &[&str]) -> RelResult<()> {
    let apt_uris = apt_uris(args).map_err(ReleaseError::AptList)?;
    let size: u64 = apt_uris.iter().map(|v| v.size).sum();

    println!("Fetching {} packages ({} MiB total)", apt_uris.len(), size / 1024 / 1024);

    let stream_of_downloads = stream::iter_ok(apt_uris);
    let buffered_stream = stream_of_downloads
        .map(move |uri| uri.fetch(&client))
        .buffer_unordered(8)
        .for_each(|v| Ok(()))
        .map_err(ReleaseError::PackageFetch);

    Runtime::new().unwrap().block_on(buffered_stream)
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
