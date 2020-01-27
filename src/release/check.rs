use crate::release_api::{ApiError, Release};
use ubuntu_version::{Codename, Version, VersionError};

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

pub struct ReleaseStatus {
    pub current: Box<str>,
    pub next:    Box<str>,
    pub build:   BuildStatus,
    is_lts:      bool,
}

impl ReleaseStatus {
    pub fn is_lts(&self) -> bool { self.is_lts }
}

pub fn check(development: bool) -> Result<ReleaseStatus, VersionError> {
    find_next_release(development, Version::detect, Release::build_exists)
}

pub fn check_current(version: Option<&str>) -> Option<(String, u16)> {
    find_current_release(Version::detect, Release::build_exists, version)
}

fn format_version(version: Version) -> String { format!("{}.{:02}", version.major, version.minor) }

fn find_current_release(
    version_detect: fn() -> Result<Version, VersionError>,
    release_exists: fn(&str, &str) -> Result<u16, ApiError>,
    version: Option<&str>,
) -> Option<(String, u16)> {
    if let Some(version) = version {
        let build = release_exists(version, "intel").ok()?;
        return Some((version.into(), build));
    }

    let mut current = version_detect().ok()?;
    let mut current_str = format_version(current);
    let mut available = release_exists(&current_str, "intel").ok()?;

    let mut next = current.next_release();
    let mut next_str = format_version(next);

    while let Ok(build) = release_exists(&next_str, "intel") {
        available = build;
        current = next;
        current_str = next_str;
        next = current.next_release();
        next_str = format_version(next);
    }

    Some((current_str, available))
}

fn find_next_release(
    development: bool,
    version_detect: fn() -> Result<Version, VersionError>,
    release_exists: fn(&str, &str) -> Result<u16, ApiError>,
) -> Result<ReleaseStatus, VersionError> {
    let current = version_detect()?;
    let mut next = current.next_release();
    let mut next_str = format_version(next);
    let mut available = release_exists(&next_str, "intel");

    if available.is_ok() {
        let mut next_next = next.next_release();
        let mut next_next_str = format_version(next_next);

        let mut last_build_status = release_exists(&next_next_str, "intel");

        loop {
            if let Ok(build) = last_build_status {
                available = Ok(build);
                next = next_next;
                next_str = next_next_str;
                next_next = next.next_release();
                next_next_str = format_version(next_next);
            } else if development {
                // If the next release is available, then the development
                // release is the release after the last available release.
                next = next.next_release();
                next_str = format_version(next);
                available = last_build_status;

                break;
            } else {
                break;
            }

            last_build_status = release_exists(&next_next_str, "intel");
        }
    }

    Ok(ReleaseStatus {
        current: format_version(current).into(),
        next:    next_str.into(),
        build:   available.into(),
        is_lts:  current.is_lts(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ubuntu_version::{Version, VersionError};

    fn v1804() -> Result<Version, VersionError> { Ok(Version { major: 18, minor: 4, patch: 0 }) }

    fn v1810() -> Result<Version, VersionError> { Ok(Version { major: 18, minor: 10, patch: 0 }) }

    fn v1904() -> Result<Version, VersionError> { Ok(Version { major: 19, minor: 4, patch: 0 }) }

    fn releases_up_to_1904(release: &str, _kind: &str) -> Result<u16, ApiError> {
        match release {
            "18.04" | "18.10" | "19.04" => Ok(1),
            _ => Err(ApiError::BuildNaN("".into())),
        }
    }

    fn releases_up_to_1910(release: &str, kind: &str) -> Result<u16, ApiError> {
        releases_up_to_1904(release, kind).or_else(|_| {
            if release == "19.10" {
                Ok(1)
            } else {
                Err(ApiError::BuildNaN("".into()))
            }
        })
    }

    #[test]
    fn release_check() {
        let mut status = find_next_release(false, v1804, releases_up_to_1910).unwrap();
        assert!("19.10" == dbg!(status.next.as_ref()) && status.build.is_ok());

        status = find_next_release(false, v1810, releases_up_to_1910).unwrap();
        assert!("19.10" == dbg!(status.next.as_ref()) && status.build.is_ok());

        status = find_next_release(false, v1810, releases_up_to_1904).unwrap();
        assert!("19.04" == dbg!(status.next.as_ref()) && status.build.is_ok());

        status = find_next_release(false, v1904, releases_up_to_1904).unwrap();
        assert!("19.10" == dbg!(status.next.as_ref()) && !status.build.is_ok());

        status = find_next_release(true, v1804, releases_up_to_1910).unwrap();
        assert!("20.04" == dbg!(status.next.as_ref()) && !status.build.is_ok());
    }

    #[test]
    fn current_release_check() {
        let (current, _build) = find_current_release(v1804, releases_up_to_1910, None).unwrap();
        assert!("19.10" == current.as_str());

        let (current, _build) = find_current_release(v1904, releases_up_to_1904, None).unwrap();
        assert!("19.04" == current.as_str());

        let (current, _build) =
            find_current_release(v1904, releases_up_to_1904, Some("18.04")).unwrap();
        assert!("18.04" == current.as_str());
    }
}
