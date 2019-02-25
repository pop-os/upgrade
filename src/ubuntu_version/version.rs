use os_release::OsRelease;
use std::fmt::{self, Display, Formatter};
use std::io;
use std::str::FromStr;

#[derive(Debug, Error)]
pub enum VersionError {
    #[error(display = "failed to fetch /etc/os-release: {}", _0)]
    OsRelease(io::Error),
    #[error(display = "release version component was not a number: found {}", _0)]
    VersionNaN(String),
    #[error(display = "invalid minor release version: expected 4 or 10, found {}", _0)]
    InvalidMinorVersion(u8),
    #[error(display = "major version does not exist")]
    NoMajor,
    #[error(display = "minor version does not exist")]
    NoMinor,
    #[error(display = "release version is empty")]
    NoVersion,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Version {
    pub major: u16,
    pub minor: u8,
    pub patch: u8,
}

impl Version {
    pub fn detect() -> Result<Self, <Self as FromStr>::Err> {
        let release = OsRelease::new().map_err(VersionError::OsRelease)?;
        release.version.parse::<Version>()
    }

    pub fn next(self) -> Self {
        let (major, minor) = if self.minor == 10 { (self.major + 1, 4) } else { (self.major, 10) };

        Version { major, minor, patch: 0 }
    }
}

impl Display for Version {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        write!(fmt, "{}.{}", self.major, self.minor)?;

        if self.patch != 0 {
            write!(fmt, "{}", self.patch)?;
        }

        Ok(())
    }
}

impl FromStr for Version {
    type Err = VersionError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let version = input.split_whitespace().next().ok_or(VersionError::NoVersion)?;
        if version.is_empty() {
            return Err(VersionError::NoVersion);
        }

        let mut iter = version.split('.');

        let major = iter.next().ok_or(VersionError::NoMajor)?;

        let major = major.parse::<u16>().map_err(|_| VersionError::VersionNaN(major.to_owned()))?;

        let minor = iter.next().ok_or(VersionError::NoMinor)?;

        let minor = minor.parse::<u8>().map_err(|_| VersionError::VersionNaN(minor.to_owned()))?;

        let patch = iter.next().and_then(|p| p.parse::<u8>().ok()).unwrap_or(0);

        Ok(Version { major, minor, patch })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            Version { major: 18, minor: 4, patch: 1 }.next()
        )
    }

    #[test]
    pub fn non_lts_parse() {
        assert_eq!(Version { major: 18, minor: 10, patch: 0 }, "18.10".parse::<Version>().unwrap())
    }

    #[test]
    pub fn non_lts_next() {
        assert_eq!(
            Version { major: 19, minor: 04, patch: 0 },
            Version { major: 18, minor: 10, patch: 0 }.next()
        )
    }
}
