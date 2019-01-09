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

use ::release_architecture::{detect_arch, ReleaseArchError};
use ::release_version::{detect_version, ReleaseVersionError};
use ::release_api::{ApiError, Release};

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
    PackageFetch(FetchError)

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

fn do_upgrade(matches: &ArgMatches) -> RelResult<()> {
    let client = Arc::new(
        Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap()
    );

    let (current, next) = detect_version()?;
    let mut upgrader = release_upgrade(client.clone(), &current, &next)?;

    fn attempt_fetch(client: Arc<Client>) -> RelResult<()> {
        let apt_uris = apt_uris(&["full-upgrade"]).map_err(ReleaseError::AptList)?;
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

fn check_root() -> RelResult<()> {
    if unsafe { libc::geteuid() } != 0 {
        Err(ReleaseError::NotRoot)
    } else {
        Ok(())
    }
}
