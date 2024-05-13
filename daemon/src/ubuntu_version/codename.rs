use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};

#[derive(Debug, thiserror::Error)]
pub enum CodenameParseError {
    #[error("unknown codename string")]
    NotFound,
}

/// The codename associated with an Ubuntu version.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Codename {
    Bionic,
    Cosmic,
    Disco,
    Eoan,
    Focal,
    Groovy,
    Hirsute,
    Impish,
    Jammy,
    Noble,
}

impl Codename {
    /// The date when this release is to be, or was, EOL'd.
    pub fn eol_date(self) -> (u32, u32, u32) {
        let (y, m, d) = self.release_date();

        if y % 2 == 0 && m == 4 {
            (y + 10, m, d)
        } else {
            (y + 1, if m == 4 { 1 } else { 7 }, d)
        }
    }

    /// Returns the release date in a `(year, month, date)` format
    pub fn release_date(self) -> (u32, u32, u32) {
        match self {
            Codename::Bionic => (2018, 4, 26),
            Codename::Cosmic => (2018, 10, 18),
            Codename::Disco => (2019, 4, 18),
            Codename::Eoan => (2019, 10, 17),
            Codename::Focal => (2020, 4, 23),
            Codename::Groovy => (2020, 10, 22),
            Codename::Hirsute => (2021, 4, 22),
            Codename::Impish => (2021, 10, 14),
            Codename::Jammy => (2022, 4, 21),
            Codename::Noble => (2024, 4, 13),
        }
    }

    /// When this was released, as the time in seconds since the Unix Epoch
    pub fn release_timestamp(self) -> u64 {
        // Create with `date "+%s" -d 10/14/2021`
        match self {
            Codename::Bionic => 1_524_700_800,
            Codename::Cosmic => 1_539_820_800,
            Codename::Disco => 1_555_545_600,
            Codename::Eoan => 1_571_270_400,
            Codename::Focal => 1_587_600_000,
            Codename::Groovy => 1_603_324_800,
            Codename::Hirsute => 1_619_071_200,
            Codename::Impish => 1_634_191_200,
            Codename::Jammy => 1_650_492_000,
            Codename::Noble => 1_712_959_200,
        }
    }
}

impl Display for Codename {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result { fmt.write_str(<&'static str>::from(*self)) }
}

impl FromStr for Codename {
    type Err = CodenameParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let release = match input {
            "bionic" => Codename::Bionic,
            "cosmic" => Codename::Cosmic,
            "disco" => Codename::Disco,
            "eoan" => Codename::Eoan,
            "focal" => Codename::Focal,
            "groovy" => Codename::Groovy,
            "hirsute" => Codename::Hirsute,
            "impish" => Codename::Impish,
            "jammy" => Codename::Jammy,
            "noble" => Codename::Noble,
            _ => return Err(CodenameParseError::NotFound),
        };

        Ok(release)
    }
}

impl From<Codename> for &'static str {
    fn from(codename: Codename) -> Self {
        match codename {
            Codename::Bionic => "bionic",
            Codename::Cosmic => "cosmic",
            Codename::Disco => "disco",
            Codename::Eoan => "eoan",
            Codename::Focal => "focal",
            Codename::Groovy => "groovy",
            Codename::Hirsute => "hirsute",
            Codename::Impish => "impish",
            Codename::Jammy => "jammy",
            Codename::Noble => "noble",
        }
    }
}
