use crate::ubuntu_version;
use std::{fs, io, path::Path, str::FromStr};
use thiserror::Error;

pub const RECOVERY_VERSION: &str = "/recovery/version";

#[derive(Debug, Error)]
pub enum RecoveryVersionError {
    #[error("build version in recovery version file is not a number")]
    BuildNaN,

    #[error("failed to read recovery version file")]
    File(#[from] io::Error),

    #[error("no build number found in recovery version file")]
    NoBuild,

    #[error("no version found in recovery version file")]
    NoVersion,

    #[error("recovery has unknown release codename")]
    Codename(#[from] ubuntu_version::CodenameParseError),

    #[error("recovery partition is corrupt")]
    Unknown,
}

#[derive(Debug, Clone)]
pub struct RecoveryVersion {
    pub version: String,
    pub build:   i16,
}

impl FromStr for RecoveryVersion {
    type Err = RecoveryVersionError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let mut iter = input.split_whitespace();
        let version = iter.next().ok_or(RecoveryVersionError::NoVersion)?;
        let build = iter
            .next()
            .ok_or(RecoveryVersionError::NoBuild)?
            .parse::<i16>()
            .map_err(|_| RecoveryVersionError::BuildNaN)?;

        Ok(RecoveryVersion { version: version.to_owned(), build })
    }
}

pub fn version() -> Result<RecoveryVersion, RecoveryVersionError> {
    if Path::new(RECOVERY_VERSION).exists() {
        recovery_file().map_err(RecoveryVersionError::File)?.parse::<RecoveryVersion>()
    } else {
        use std::io::{BufRead, BufReader};
        use ubuntu_version::{Codename, Version};

        for line in BufReader::new(std::fs::File::open("/recovery/dists/stable/Release")?).lines() {
            let line = line?;
            if line.starts_with("Codename:") {
                if let Some(codename) = line.split_ascii_whitespace().nth(1) {
                    return Ok(RecoveryVersion {
                        version: Version::from(Codename::from_str(codename)?).to_string(),
                        build:   0,
                    });
                }
            }
        }

        Err(RecoveryVersionError::Unknown)
    }
}

pub fn recovery_file() -> io::Result<String> { fs::read_to_string(RECOVERY_VERSION) }
