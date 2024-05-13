use chrono::{Datelike, Utc};
use os_release::OsRelease;
use std::{
    fmt::{self, Display, Formatter},
    io,
    str::FromStr,
};

#[derive(Debug, thiserror::Error)]
pub enum VersionError {
    #[error("failed to fetch /etc/os-release: {}", _0)]
    OsRelease(io::Error),
    #[error("version parsing error: {}", _0)]
    Parse(VersionParseError),
}

#[derive(Debug, thiserror::Error)]
pub enum VersionParseError {
    #[error("release version component was not a number: found {}", _0)]
    VersionNaN(String),
    #[error("invalid minor release version: expected 4 or 10, found {}", _0)]
    InvalidMinorVersion(u8),
    #[error("major version does not exist")]
    NoMajor,
    #[error("minor version does not exist")]
    NoMinor,
    #[error("release version is empty")]
    NoVersion,
}

/// The version of an Ubuntu release, which is based on the date of release.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Version {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
}

impl Version {
    /// Reads the `/etc/os-release` file and determines which version of Ubuntu is in use.
    pub fn detect() -> Result<Self, VersionError> {
        let release = OsRelease::new().map_err(VersionError::OsRelease)?;
        release.version.parse::<Version>().map_err(VersionError::Parse)
    }

    /// Returns `true` if this is a LTS release.
    pub fn is_lts(self) -> bool { self.major % 2 == 0 && self.minor == 4 }

    /// The number of months that have passed since this version was released.
    pub fn months_since(self) -> i32 {
        let today = Utc::today();

        let major = 2000 - today.year() as u32;
        let minor = today.month() as u32;

        months_since(self, major, minor)
    }

    /// Increments the major / minor version to the next expected release version.
    pub fn next_release(self) -> Self {
        let (major, minor) = if self.minor == 10 { (self.major + 1, 4) } else { (self.major, 10) };

        Version { major, minor, patch: 0 }
    }
}

impl Display for Version {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        write!(fmt, "{}.{:02}", self.major, self.minor)?;

        if self.patch != 0 {
            write!(fmt, "{}", self.patch)?;
        }

        Ok(())
    }
}

impl FromStr for Version {
    type Err = VersionParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let version = input.split_whitespace().next().ok_or(VersionParseError::NoVersion)?;
        if version.is_empty() {
            return Err(VersionParseError::NoVersion);
        }

        let mut iter = version.split('.');

        let major = iter.next().ok_or(VersionParseError::NoMajor)?;
        let major =
            major.parse::<u8>().map_err(|_| VersionParseError::VersionNaN(major.to_owned()))?;
        let minor = iter.next().ok_or(VersionParseError::NoMinor)?;
        let minor =
            minor.parse::<u8>().map_err(|_| VersionParseError::VersionNaN(minor.to_owned()))?;
        let patch = iter.next().and_then(|p| p.parse::<u8>().ok()).unwrap_or(0);

        Ok(Version { major, minor, patch })
    }
}

fn months_since(version: Version, major: u32, minor: u32) -> i32 {
    ((major as i32 - version.major as i32) * 12) + minor as i32 - version.minor as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months_since_release() {
        assert_eq!(18, months_since(Version { major: 18, minor: 4, patch: 0 }, 19, 10));
        assert_eq!(3, months_since(Version { major: 19, minor: 10, patch: 0 }, 20, 1));
        assert_eq!(-3, months_since(Version { major: 18, minor: 4, patch: 0 }, 18, 1))
    }

    #[test]
    pub fn lts_check() {
        assert!(Version { major: 18, minor: 4, patch: 0 }.is_lts());
        assert!(!Version { major: 18, minor: 10, patch: 0 }.is_lts());
        assert!(!Version { major: 19, minor: 4, patch: 0 }.is_lts());
        assert!(!Version { major: 19, minor: 10, patch: 0 }.is_lts());
        assert!(Version { major: 20, minor: 4, patch: 0 }.is_lts());
    }

    #[test]
    pub fn lts_parse() {
        assert_eq!(
            Version { major: 18, minor: 4, patch: 1 },
            "18.04.1 LTS".parse::<Version>().unwrap()
        )
    }

    #[test]
    pub fn lts_next() {
        assert_eq!(
            Version { major: 18, minor: 10, patch: 0 },
            Version { major: 18, minor: 4, patch: 1 }.next_release()
        )
    }

    #[test]
    pub fn non_lts_parse() {
        assert_eq!(Version { major: 18, minor: 10, patch: 0 }, "18.10".parse::<Version>().unwrap())
    }

    #[test]
    pub fn non_lts_next() {
        assert_eq!(
            Version { major: 19, minor: 4, patch: 0 },
            Version { major: 18, minor: 10, patch: 0 }.next_release()
        )
    }
}
