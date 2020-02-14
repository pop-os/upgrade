use crate::api::{ApiError, Release};
use http::StatusCode;
use std::future::Future;
use ubuntu_version::{Version, VersionError};

#[derive(Debug)]
pub enum BuildStatus {
    Blacklisted,
    Build(u16),
    ConnectionIssue(isahc::Error),
    InternalIssue(ApiError),
    ServerStatus(StatusCode),
}

impl BuildStatus {
    pub fn is_ok(&self) -> bool {
        if let BuildStatus::Build(_) = *self {
            true
        } else {
            false
        }
    }

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
            Err(ApiError::Server(status)) => BuildStatus::ServerStatus(status),
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

    let next = next_(current, development, |build| {
        let build = Box::from(build);
        async move { Release::build_exists(&*build, "intel").await.into() }
    });

    Ok(next.await)
}

pub async fn current(version: Option<&str>) -> Option<(Box<str>, u16)> {
    if let Some(version) = version {
        let build = Release::build_exists(version, "intel").await.ok()?;
        return Some((version.into(), build));
    }

    let current = Version::detect().ok()?;
    let release_str = release_str(current.major, current.minor);

    Some((release_str.into(), Release::build_exists(release_str, "intel").await.ok()?))
}

pub fn release_str(major: u8, minor: u8) -> &'static str {
    match (major, minor) {
        (18, 4) => "18.04",
        (19, 10) => "18.10",
        (20, 4) => "20.04",
        _ => panic!("this version of pop-upgrade is not supported on this release"),
    }
}

async fn next_<F: Future<Output = BuildStatus>>(
    current: Version,
    development: bool,
    release_check: impl for<'a> Fn(&'a str) -> F,
) -> ReleaseStatus {
    let next: &str;
    match (current.major, current.minor) {
        (18, 4) => {
            next = if development { "20.04" } else { "19.10" };

            ReleaseStatus { build: release_check(next).await, current: "18.04", is_lts: true, next }
        }

        (19, 10) => {
            next = "20.04";

            ReleaseStatus {
                build: if development {
                    release_check(next).await
                } else {
                    BuildStatus::Blacklisted
                },
                current: "19.10",
                is_lts: false,
                next,
            }
        }

        (20, 4) => ReleaseStatus {
            build:   BuildStatus::Blacklisted,
            current: "20.04",
            is_lts:  true,
            next:    "20.10",
        },
        _ => panic!("this version of pop-upgrade is not supported on this release"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_next() {
        let bionic = Version { major: 18, minor: 4, patch: 0 };
        let eoan = Version { major: 19, minor: 10, patch: 0 };

        futures::executor::block_on(async move {
            assert_eq!(
                next_(bionic, false, |_| async { BuildStatus::Build(1) }).await,
                ReleaseStatus {
                    current: "18.04",
                    next:    "19.10",
                    build:   BuildStatus::Build(1),
                    is_lts:  true,
                }
            );

            assert_eq!(
                next_(bionic, true, |_| async { BuildStatus::Build(1) }).await,
                ReleaseStatus {
                    current: "18.04",
                    next:    "20.04",
                    build:   BuildStatus::Build(1),
                    is_lts:  true,
                }
            );

            assert_eq!(
                next_(eoan, false, |_| async { BuildStatus::Build(1) }).await,
                ReleaseStatus {
                    current: "19.10",
                    next:    "20.04",
                    build:   BuildStatus::Blacklisted,
                    is_lts:  false,
                }
            );

            assert_eq!(
                next_(eoan, true, |_| async { BuildStatus::Build(1) }).await,
                ReleaseStatus {
                    current: "19.10",
                    next:    "20.04",
                    build:   BuildStatus::Build(1),
                    is_lts:  false,
                }
            );
        });
    }
}
