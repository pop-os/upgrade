use crate::{
    release_api::{ApiError, Release},
    ubuntu_version::{Version, VersionError},
};
use anyhow::Context;
use std::future::Future;

#[derive(Debug)]
pub enum BuildStatus {
    Blacklisted,
    Build(u16),
    ConnectionIssue(isahc::Error),
    InternalIssue(ApiError),
    ServerStatus(isahc::http::StatusCode),
}

impl BuildStatus {
    pub fn is_ok(&self) -> bool { matches!(*self, BuildStatus::Build(_)) }

    pub fn status_code(&self) -> i16 {
        match *self {
            BuildStatus::ConnectionIssue(_) => -3,
            BuildStatus::ServerStatus(_) => -2,
            BuildStatus::InternalIssue(_) => -1,
            BuildStatus::Build(build) => build as i16,
            BuildStatus::Blacklisted => -4,
        }
    }
}

impl From<Result<u16, ApiError>> for BuildStatus {
    fn from(result: Result<u16, ApiError>) -> Self {
        match result {
            Err(ApiError::Get(why)) => BuildStatus::ConnectionIssue(why),
            Err(ApiError::Status(why)) => BuildStatus::ServerStatus(why),
            Err(otherwise) => BuildStatus::InternalIssue(otherwise),
            Ok(build) => BuildStatus::Build(build),
        }
    }
}

impl PartialEq for BuildStatus {
    fn eq(&self, other: &BuildStatus) -> bool {
        match (self, other) {
            (BuildStatus::Blacklisted, BuildStatus::Blacklisted)
            | (BuildStatus::ConnectionIssue(_), BuildStatus::ConnectionIssue(_))
            | (BuildStatus::InternalIssue(_), BuildStatus::InternalIssue(_))
            | (BuildStatus::ServerStatus(_), BuildStatus::ServerStatus(_)) => true,
            (BuildStatus::Build(a), BuildStatus::Build(b)) => a == b,
            _ => false,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct ReleaseStatus {
    pub current: &'static str,
    pub next:    &'static str,
    pub build:   BuildStatus,
    pub is_lts:  bool,
}

impl ReleaseStatus {
    pub fn is_lts(&self) -> bool { self.is_lts }
}

pub async fn next(development: bool) -> Result<ReleaseStatus, VersionError> {
    let current = Version::detect()?;

    Ok(next_(current, development, &|build: String| async move {
        Release::build_exists(&build, "intel").await.into()
    })
    .await)
}

pub async fn current(version: Option<&str>) -> anyhow::Result<(Box<str>, u16)> {
    info!("Checking for current release of {:?}", version);

    if let Some(version) = version {
        let build = Release::build_exists(version, "intel")
            .await
            .with_context(|| fomat!("failed to find build for "(version)))?;

        return Ok((version.into(), build));
    }

    let current = Version::detect().context("cannot detect current version of Pop")?;
    let release_str = release_str(current.major, current.minor);

    let build = Release::build_exists(release_str, "intel")
        .await
        .with_context(|| fomat!("failed to find build for "(release_str)))?;

    Ok((release_str.into(), build))
}

const BIONIC: &str = "18.04";
const FOCAL: &str = "20.04";
const GROOVY: &str = "20.10";
const HIRSUTE: &str = "21.04";
const IMPISH: &str = "21.10";
const JAMMY: &str = "22.04";
const NOBLE: &str = "24.04";
const UNKNOWN: &str = "26.04";

pub fn release_str(major: u8, minor: u8) -> &'static str {
    match (major, minor) {
        (18, 4) => BIONIC,
        (20, 4) => FOCAL,
        (20, 10) => GROOVY,
        (21, 4) => HIRSUTE,
        (21, 10) => IMPISH,
        (22, 4) => JAMMY,
        (24, 4) => NOBLE,
        (26, 4) => UNKNOWN,
        _ => panic!("this version of pop-upgrade is not supported on this release"),
    }
}

async fn next_<Check: Fn(String) -> Status, Status: Future<Output = BuildStatus>>(
    current: Version,
    development: bool,
    release_check: &Check,
) -> ReleaseStatus {
    // Enables a release upgrade from current to next, if a next ISO exists
    let available = |is_lts: bool, current: &'static str, next: &'static str| async move {
        ReleaseStatus { build: release_check(next.into()).await, current, is_lts, next }
    };

    // Disables any form of upgrades from occurring on this release
    let blocked = |is_lts: bool, current: &'static str, next: &'static str| async move {
        ReleaseStatus { build: BuildStatus::Blacklisted, current, is_lts, next }
    };

    // Only permits an upgrade if the development flag is passed
    let development_enabled = |is_lts: bool, current: &'static str, next: &'static str| async move {
        let build =
            if development { release_check(next.into()).await } else { BuildStatus::Blacklisted };
        ReleaseStatus { current, next, build, is_lts }
    };

    match (current.major, current.minor) {
        (18, 4) => available(true, BIONIC, FOCAL).await,
        (20, 4) => available(true, FOCAL, JAMMY).await,
        (20, 10) => available(false, GROOVY, HIRSUTE).await,
        (21, 4) => available(false, HIRSUTE, IMPISH).await,
        (21, 10) => available(false, IMPISH, JAMMY).await,
        (22, 4) => development_enabled(true, JAMMY, NOBLE).await,
        (24, 4) => blocked(true, NOBLE, UNKNOWN).await,
        _ => panic!("this version of pop-upgrade is not supported on this release"),
    }
}
