//! Check for the last urgent release

use crate::misc::parse_rfc2822;

use async_std::fs;
use chrono::{DateTime, FixedOffset};
use http::status::StatusCode;
use isahc::prelude::*;
use std::{
    convert::TryFrom,
    io,
    path::Path,
    time::{Duration, SystemTime},
};
use thiserror::Error;

const API_URI: &str = "https://raw.githubusercontent.com/pop-os/upgrade/refresh-os/api/";

pub fn release_uri(release: &str, variant: &str) -> String {
    fomat!((API_URI) (release) "/" (variant) "/release.ron")
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("failed to fetch due to client error")]
    Client(StatusCode),

    #[error("failed to fetch release from endpoint")]
    Get(#[source] isahc::Error),

    #[error("failed to deserialize release")]
    Parse(#[source] anyhow::Error),

    #[error("failed to fetch due to server error")]
    Server(StatusCode),

    #[error("failed to fetch text from release endpoint")]
    TextFetch(#[source] io::Error),
}

#[derive(Debug)]
pub struct Release {
    pub build:  Option<Build>,
    pub urgent: Option<Urgent>,
}

impl TryFrom<RawRelease> for Release {
    type Error = ApiError;

    fn try_from(raw: RawRelease) -> Result<Self, Self::Error> {
        let urgent = match raw.urgent {
            Some(urgent) => {
                let date = parse_rfc2822(&*urgent.date).map_err(ApiError::Parse)?;
                Some(Urgent { date, build: urgent.build })
            }
            None => None,
        };

        Ok(Release { build: raw.build, urgent })
    }
}

impl Release {
    pub async fn fetch(release: &str, variant: &str) -> Result<Self, ApiError> {
        const CACHE_DIR: &str = "/var/cache/pop-upgrade";
        const CACHE: &str = "/var/cache/pop-upgrade/release";

        if Path::new(CACHE).exists() && !requires_refresh(CACHE).await {
            if let Ok(text) = fs::read_to_string(CACHE).await {
                let release = ron::de::from_str::<RawRelease>(&text)
                    .map_err(|why| ApiError::Parse(why.into()))
                    .and_then(Release::try_from);

                if let Ok(release) = release {
                    return Ok(release);
                }
            }
        }

        let text = Self::fetch_text(release, variant).await?;

        let release = ron::de::from_str::<RawRelease>(&text)
            .map_err(|why| ApiError::Parse(why.into()))
            .and_then(Release::try_from)?;

        let _ = fs::create_dir_all(CACHE_DIR).await;
        let _ = fs::write(CACHE, text.as_bytes()).await;

        Ok(release)
    }

    pub async fn build_exists(release: &str, variant: &str) -> Result<u16, ApiError> {
        Self::fetch(release, variant)
            .await
            .map(|release| release.build.map_or(0, |build| build.build))
    }

    async fn fetch_text(release: &str, variant: &str) -> Result<String, ApiError> {
        let release = release_uri(release, variant);

        info!("fetching release info from '{}'", release);

        let mut resp = isahc::get_async(&release).await.map_err(ApiError::Get)?;

        let status = resp.status();

        if status.is_client_error() {
            return Err(ApiError::Client(status));
        } else if status.is_server_error() {
            return Err(ApiError::Server(status));
        }

        let text = resp.text_async().await.map_err(ApiError::TextFetch)?;
        info!("fetched release: {}", text);
        Ok(text)
    }
}
#[derive(Deserialize)]
struct RawRelease {
    #[serde(default)]
    pub build:  Option<Build>,
    #[serde(default)]
    pub urgent: Option<RawUrgent>,
}

#[derive(Debug, Deserialize)]
pub struct Build {
    pub url:   Box<str>,
    pub sha:   Box<str>,
    pub size:  u64,
    pub build: u16,
}

#[derive(Deserialize)]
pub struct RawUrgent {
    pub date:  Box<str>,
    pub build: u16,
}

#[derive(Debug)]
pub struct Urgent {
    pub date:  DateTime<FixedOffset>,
    pub build: u16,
}

async fn requires_refresh(path: &str) -> bool {
    if let Ok(metadata) = fs::metadata(path).await {
        if let Ok(modified) = metadata.modified() {
            let current = SystemTime::now();
            if let Ok(since) = current.duration_since(modified) {
                if since < Duration::from_secs(60 * 60 * 24) {
                    return false;
                }
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"#![enable(implicit_some)]
(
	build: (
		url: "https://pop-iso.sfo2.cdn.digitaloceanspaces.com/18.04/amd64/intel/59/pop-os_18.04_amd64_intel_59.iso",
		sha: "0ae2c20327bc1059892c9efea71b21753782979431091fa3da60e4eaa036db1c",
		size: 2256076800,
		build: 59
	),
	urgent: (
		date: "Fri, 31 Jan 2020 20:46:23 +0000",
		build: 59,
	)
)"#;

    const EXAMPLE_2: &str = r#"#![enable(implicit_some)]
()"#;

    #[test]
    fn test_example() { ron::de::from_str::<RawRelease>(EXAMPLE).unwrap(); }

    #[test]
    fn test_example2() { ron::de::from_str::<RawRelease>(EXAMPLE_2).unwrap(); }
}
