pub enum UbuntuCodename {
    Bionic,
    Cosmic,
    Disco,
}

impl UbuntuCodename {
    pub fn from_version(version: &str) -> Option<Self> {
        let release = match version {
            "18.04" => UbuntuCodename::Bionic,
            "18.10" => UbuntuCodename::Cosmic,
            "19.04" => UbuntuCodename::Disco,
            _ => return None,
        };

        Some(release)
    }

    pub fn into_codename(self) -> &'static str {
        match self {
            UbuntuCodename::Bionic => "bionic",
            UbuntuCodename::Cosmic => "cosmic",
            UbuntuCodename::Disco => "disco",
        }
    }

    // pub fn into_version(self) -> &'static str {
    //     match self {
    //         UbuntuCodename::Bionic => "18.04",
    //         UbuntuCodename::Cosmic => "18.10",
    //         UbuntuCodename::Disco => "19.04"
    //     }
    // }
}
