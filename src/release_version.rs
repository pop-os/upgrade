use os_release::OsRelease;
use std::io;

#[derive(Debug, Error)]
pub enum ReleaseVersionError {
    #[error(display = "failed to fetch /etc/os-release: {}", _0)]
    OsRelease(io::Error),
    #[error(display = "release version component was not a number: found {}", _0)]
    VersionNaN(String),
    #[error(display = "invalid minor release version: expected 4 or 10, found {}", _0)]
    InvalidMinorVersion(u8),
    #[error(display = "minor version does not exist in {}", _0)]
    NoMinor(String)
}

pub fn detect_version() -> Result<(String, String), ReleaseVersionError> {
    let release = OsRelease::new().map_err(ReleaseVersionError::OsRelease)?;

    let (major, minor) = match release.version.find('.') {
        Some(position) => {
            let (major, mut minor) = release.version.split_at(position);
            minor = &minor[1..];

            let major = match major.parse::<u8>() {
                Ok(major) => major,
                Err(_) => return Err(ReleaseVersionError::VersionNaN(major.to_owned()))
            };

            let minor = match minor.parse::<u8>() {
                Ok(minor) => minor,
                Err(_) => return Err(ReleaseVersionError::VersionNaN(minor.to_owned()))
            };

            (major, minor)
        }
        None => return Err(ReleaseVersionError::NoMinor(release.version.to_owned()))
    };

    let (new_major, new_minor) = match minor {
        4 => (major, minor + 6),
        10 => (major + 1, 4),
        _ => return Err(ReleaseVersionError::InvalidMinorVersion(minor))
    };

    Ok((release.version.to_owned(), format!("{}.{:02}", new_major, new_minor)))
}
