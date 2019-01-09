use async_fetcher::{AsyncFetcher, FetchError, FetchEvent};
use command::Command;
use reqwest::r#async::Client;
use std::io;
use std::str::FromStr;
use std::sync::Arc;
use std::path::Path;
use md5::Md5;
use futures::{Future, IntoFuture};

pub fn apt_uris() -> Result<Vec<AptUri>, AptUriError> {
    let output = Command::new("apt-get")
        .env("DEBIAN_FRONTEND", "noninteractive")
        .args(&["--print-uris", "full-upgrade"])
        .run_with_stdout()
        .map_err(AptUriError::Command)?;

    let mut packages = Vec::new();
    for line in output.lines() {
        if !line.starts_with('\'') {
            continue
        }

        packages.push(line.parse::<AptUri>()?);
    }

    Ok(packages)
}

#[derive(Debug, Error)]
pub enum AptUriError {
    #[error(display = "apt command failed: {}", _0)]
    Command(io::Error),
    #[error(display = "uri not found in output: {}", _0)]
    UriNotFound(String),
    #[error(display = "invalid URI value: {}", _0)]
    UriInvalid(String),
    #[error(display = "name not found in output: {}", _0)]
    NameNotFound(String),
    #[error(display = "size not found in output: {}", _0)]
    SizeNotFound(String),
    #[error(display = "size in output could not be parsed as an integer: {}", _0)]
    SizeParse(String),
    #[error(display = "md5sum not found in output: {}", _0)]
    Md5NotFound(String),
    #[error(display = "md5 prefix (MD5Sum:) not found in md5sum: {}", _0)]
    Md5Prefix(String)
}

#[derive(Debug, Clone)]
pub struct AptUri {
    pub uri: String,
    pub name: String,
    pub size: u64,
    pub md5sum: String
}

impl AptUri {
    pub fn fetch(self, client: &Client) -> impl Future<Item = Self, Error = FetchError> {
        const ARCHIVES: &str = "/var/cache/apt/archives/";
        const PARTIAL: &str = "/var/cache/apt/archives/partial/";

        let dest: Arc<Path> = Path::new(ARCHIVES).join(&self.name).into();
        let part: Arc<Path> = Path::new(PARTIAL).join(&self.name).into();

        let name = self.name.clone();

        AsyncFetcher::new(client, self.uri.clone())
            .with_progress_callback(move |event| match event {
                FetchEvent::Get => println!("Getting {}", name),
                FetchEvent::DownloadComplete
                    | FetchEvent::AlreadyFetched => println!("Finished {}", name),
                _ => ()
            })
            .request_to_path_with_checksum::<Md5>(dest, &self.md5sum.clone())
            .then_download(part)
            .then_rename()
            .into_future()
            .map(move |_| self)
    }
}

impl FromStr for AptUri {
    type Err = AptUriError;

    fn from_str(line: &str) -> Result<Self, Self::Err> {
        let mut words = line.split_whitespace();

        let mut uri = words.next().ok_or_else(|| AptUriError::UriNotFound(line.into()))?;

        // We need to remove the single quotes that apt-get encloses the URI within.
        if uri.len() <= 3 {
            return Err(AptUriError::UriInvalid(uri.into()))
        } else {
            uri = &uri[1..uri.len() - 1];
        }

        let name = words.next().ok_or_else(|| AptUriError::NameNotFound(line.into()))?;
        let size = words.next().ok_or_else(|| AptUriError::SizeNotFound(line.into()))?;
        let size = size.parse::<u64>().map_err(|_| AptUriError::SizeParse(size.into()))?;
        let mut md5sum = words.next().ok_or_else(|| AptUriError::Md5NotFound(line.into()))?;

        if md5sum.starts_with("MD5Sum:") {
            md5sum = &md5sum[7..];
        } else {
            return Err(AptUriError::Md5Prefix(md5sum.into()));
        }

        Ok(AptUri { uri: uri.into(), name: name.into(), size, md5sum: md5sum.into() })
    }
}
