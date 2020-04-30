use crate::release_api::{ApiError, Release};
use ubuntu_version::{Version, VersionError};

#[derive(Debug)]
pub enum BuildStatus {
    Blacklisted,
    Build(u16),
    ConnectionIssue(reqwest::Error),
    InternalIssue(ApiError),
    ServerStatus(reqwest::Error),
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

pub fn next(development: bool) -> Result<ReleaseStatus, VersionError> {
    Version::detect().map(|current| {
        next_(current, development, |build| Release::build_exists(build, "intel").into())
    })
}

pub fn current(version: Option<&str>) -> Option<(Box<str>, u16)> {
    if let Some(version) = version {
        let build = Release::build_exists(version, "intel").ok()?;
        return Some((version.into(), build));
    }

    let current = Version::detect().ok()?;
    let release_str = release_str(current.major, current.minor);

    Some((release_str.into(), Release::build_exists(release_str, "intel").ok()?))
}

pub fn release_str(major: u8, minor: u8) -> &'static str {
    match (major, minor) {
        (18, 4) => "18.04",
        (19, 10) => "18.10",
        (20, 4) => "20.04",
        _ => panic!("this version of pop-upgrade is not supported on this release"),
    }
}

fn next_(
    current: Version,
    development: bool,
    release_check: impl Fn(&str) -> BuildStatus,
) -> ReleaseStatus {
    let next: &str;
    match (current.major, current.minor) {
        (18, 4) => {
            // next = if development { "20.10" } else { "20.04" };
            next = "20.04";

            ReleaseStatus { build: release_check(next), current: "18.04", is_lts: true, next }
        }

        (19, 10) => {
            next = "20.04";

            ReleaseStatus { build: release_check(next), current: "19.10", is_lts: false, next }
        }

        (20, 4) => {
            next = "20.10";

            ReleaseStatus {
                build: if development { release_check(next) } else { BuildStatus::Blacklisted },
                current: "20.04",
                is_lts: true,
                next,
            }
        }

        (20, 10) => ReleaseStatus {
            build:   BuildStatus::Blacklisted,
            current: "20.10",
            is_lts:  true,
            next:    "21.04",
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

        assert_eq!(
            next_(bionic, false, |_| BuildStatus::Build(1)),
            ReleaseStatus {
                current: "18.04",
                next:    "19.10",
                build:   BuildStatus::Build(1),
                is_lts:  true,
            }
        );

        assert_eq!(
            next_(bionic, true, |_| BuildStatus::Build(1)),
            ReleaseStatus {
                current: "18.04",
                next:    "20.04",
                build:   BuildStatus::Build(1),
                is_lts:  true,
            }
        );

        assert_eq!(
            next_(eoan, false, |_| BuildStatus::Build(1)),
            ReleaseStatus {
                current: "19.10",
                next:    "20.04",
                build:   BuildStatus::Blacklisted,
                is_lts:  false,
            }
        );

        assert_eq!(
            next_(eoan, true, |_| BuildStatus::Build(1)),
            ReleaseStatus {
                current: "19.10",
                next:    "20.04",
                build:   BuildStatus::Build(1),
                is_lts:  false,
            }
        );
    }
}
