use apt_fetcher::{DistUpgradeError, UpgradeRequest, SourcesList};
use apt_keyring::AptKeyring;
use clap::ArgMatches;
use futures::{stream, Stream};
use reqwest::r#async::Client;
use tokio::runtime::Runtime;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ::apt_uris::{apt_uris, AptUri, AptUriError};
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
    Overwrite(DistUpgradeError)
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

pub fn release(matches: &ArgMatches) -> Result<(), ReleaseError> {
    match matches.subcommand() {
        ("check", _) => check_release()?,
        ("fetch", Some(matches)) => fetch_packages(matches)?,
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

fn fetch_packages(matches: &ArgMatches) -> Result<(), ReleaseError> {
    let client = Arc::new(
        Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap()
    );

    let (current, next) = detect_version()?;
    release_upgrade(client.clone(), &current, &next)?;

    let stream_of_downloads = stream::iter_ok(apt_uris().map_err(ReleaseError::AptList)?);

    let buffered_stream = stream_of_downloads
        .map(move |uri| uri.fetch(&client))
        .buffer_unordered(8);

    let results = Runtime::new().unwrap().block_on(buffered_stream.collect());
    println!("completed: {:#?}", results);

    Ok(())
}

fn do_upgrade(matches: &ArgMatches) -> Result<(), ReleaseError> {
    let (current, next) = detect_version()?;
    // release_upgrade(&current, &next)?;

    Ok(())
}

/// Check if release files can be upgraded, and then overwrite them with the new release.
fn release_upgrade(client: Arc<Client>, current: &str, new: &str) -> Result<(), ReleaseError> {
    let current = UbuntuCodename::from_version(current).map_or(current, |c| c.as_codename());
    let new = UbuntuCodename::from_version(new).map_or(new, |c| c.as_codename());

    let sources = SourcesList::scan().unwrap();
    let client = Arc::new(Client::new());
    let keyring = Arc::new(AptKeyring::new().unwrap());

    println!("Checking if release can be upgraded from {} to {}", current, new);
    let mut upgrade = UpgradeRequest::new(client, sources)
        .keyring(keyring)
        .send(current, new)
        .map_err(ReleaseError::Check)?;

    println!("Upgrade is possible -- updating release files");
    upgrade.overwrite_apt_sources()
        .map_err(ReleaseError::Overwrite)
}
