use std::str::FromStr;
use std::{fs, io};

pub const RECOVERY_VERSION: &str = "/recovery/version";

#[derive(Debug, Error)]
pub enum RecoveryVersionError {
    #[error(display = "build version in recovery version file is not a number")]
    BuildNaN,
    #[error(display = "failed to read recovery version file: {}", _0)]
    File(io::Error),
    #[error(display = "no build number found in recovery version file")]
    NoBuild,
    #[error(display = "no version found in recovery version file")]
    NoVersion,
}

#[derive(Debug, Clone)]
pub struct RecoveryVersion {
    pub version: String,
    pub build: u16
}

impl FromStr for RecoveryVersion {
    type Err = RecoveryVersionError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let mut iter = input.split_whitespace();
        let version = iter.next().ok_or(RecoveryVersionError::NoVersion)?;
        let build = iter.next()
            .ok_or(RecoveryVersionError::NoBuild)?
            .parse::<u16>()
            .map_err(|_| RecoveryVersionError::BuildNaN)?;

        Ok(RecoveryVersion { version: version.to_owned(), build })
    }
}

pub fn version() -> Result<RecoveryVersion, RecoveryVersionError> {
    recovery_file().map_err(RecoveryVersionError::File)?
        .parse::<RecoveryVersion>()
}

pub fn recovery_file() -> io::Result<String> {
    fs::read_to_string(RECOVERY_VERSION)
}
