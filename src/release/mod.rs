use clap::ArgMatches;
use futures::{stream, Stream};
use reqwest::r#async::Client;
use tokio::runtime::Runtime;

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
    AptList(AptUriError)
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

fn fetch_packages(matches: &ArgMatches) -> Result<(), ReleaseError> {
    let client = Client::new();

    let stream_of_downloads = stream::iter_ok(apt_uris().map_err(ReleaseError::AptList)?);

    let buffered_stream = stream_of_downloads
        .map(move |uri| uri.fetch(&client))
        .buffer_unordered(8);

    let results = Runtime::new().unwrap().block_on(buffered_stream.collect());
    println!("completed: {:#?}", results);

    Ok(())
}

fn do_upgrade(matches: &ArgMatches) -> Result<(), ReleaseError> {
    Ok(())
}
