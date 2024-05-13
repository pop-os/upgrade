mod codename;
mod version;

pub use self::{codename::*, version::*};
use std::convert::TryFrom;

impl TryFrom<Version> for Codename {
    type Error = ();

    fn try_from(version: Version) -> Result<Self, Self::Error> {
        Ok(match (version.major, version.minor) {
            (18, 4) => Codename::Bionic,
            (18, 10) => Codename::Cosmic,
            (19, 4) => Codename::Disco,
            (19, 10) => Codename::Eoan,
            (20, 4) => Codename::Focal,
            (20, 10) => Codename::Groovy,
            (21, 4) => Codename::Hirsute,
            (21, 10) => Codename::Impish,
            (22, 4) => Codename::Jammy,
            (24, 4) => Codename::Noble,
            _ => return Err(()),
        })
    }
}

impl From<Codename> for Version {
    fn from(codename: Codename) -> Version {
        let (major, minor) = match codename {
            Codename::Bionic => (18, 4),
            Codename::Cosmic => (18, 10),
            Codename::Disco => (19, 4),
            Codename::Eoan => (19, 10),
            Codename::Focal => (20, 4),
            Codename::Groovy => (20, 10),
            Codename::Hirsute => (21, 4),
            Codename::Impish => (21, 10),
            Codename::Jammy => (22, 4),
            Codename::Noble => (24, 4),
        };

        Version { major, minor, patch: 0 }
    }
}
