use crate::{
    api::{Release, Urgent},
    misc::parse_rfc2822,
};
use anyhow::Context;
use async_std::fs;
use chrono::{DateTime, FixedOffset};
use futures::try_join;

const RECOVERY_RELEASE: &str = "/recovery/dists/stable/Release";

/// Contents of the Release file in the recovery partition as a string
#[derive(Debug)]
pub struct RecoveryRelease {
    pub date:    DateTime<FixedOffset>,
    pub version: Box<str>,
}

impl RecoveryRelease {
    pub async fn fetch() -> anyhow::Result<Self> {
        let release = fs::read_to_string(RECOVERY_RELEASE)
            .await
            .context("failed to fetch release file from recovery partition")?;

        let (mut date, mut version) = (None, None);

        for line in release.lines() {
            if date.is_none() {
                if let Some(field) = parse_field(line, "Date:") {
                    date = Some(parse_rfc2822(field)?);
                    if version.is_some() {
                        break;
                    }
                }
            }

            if version.is_none() {
                if let Some(field) = parse_field(line, "Version:") {
                    version = Some(field.into());
                    if date.is_some() {
                        break;
                    }
                }
            }
        }

        let error = match (date, version) {
            (Some(date), Some(version)) => {
                let release = Self { date, version };
                info!("{:?}", release);
                return Ok(release);
            }
            (None, None) => "missing date and version fields",
            (None, _) => "missing date field",
            (_, None) => "missing version field",
        };

        Err(anyhow!(error))
    }

    pub async fn urgent_check(version: &str, variant: &str) -> anyhow::Result<Option<Urgent>> {
        let current =
            async { Self::fetch().await.context("failed to get recovery partition release info") };

        let release = async move {
            Release::fetch(version, variant).await.context("release date fetch failed")
        };

        try_join!(current, release).map(|(current, release)| {
            release.urgent.and_then(|urgent| {
                if version != &*current.version || current.date < urgent.date {
                    Some(urgent)
                } else {
                    None
                }
            })
        })
    }
}

fn parse_field<'a>(line: &'a str, pattern: &str) -> Option<&'a str> {
    if line.starts_with(pattern) {
        Some(&line[pattern.len() + 1..])
    } else {
        None
    }
}
