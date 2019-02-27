use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AptUpgradeEvent {
    Processing { package: String },
    Progress { percent: u8 },
    SettingUp { package: String },
    Unpacking { package: String, version: String, over: String },
}

impl AptUpgradeEvent {
    pub fn to_dbus_map(self) -> HashMap<&'static str, String> {
        let mut map = HashMap::new();

        match self {
            AptUpgradeEvent::Processing { package } => {
                map.insert("processing_package", package);
            }
            AptUpgradeEvent::Progress { percent } => {
                map.insert("percent", percent.to_string());
            }
            AptUpgradeEvent::SettingUp { package } => {
                map.insert("setting_up", package);
            }
            AptUpgradeEvent::Unpacking { package, version, over } => {
                map.insert("unpacking", package);
                map.insert("version", version);
                map.insert("over", over);
            }
        }

        map
    }
}

impl Display for AptUpgradeEvent {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        match self {
            AptUpgradeEvent::Processing { package } => {
                write!(fmt, "processing triggers for {}", package)
            }
            AptUpgradeEvent::Progress { percent } => write!(fmt, "progress: [{:03}%]", percent),
            AptUpgradeEvent::SettingUp { package } => write!(fmt, "setting up {}", package),
            AptUpgradeEvent::Unpacking { package, version, over } => {
                write!(fmt, "unpacking {} ({}) over ({})", package, version, over)
            }
        }
    }
}

// TODO: Unit test this
impl FromStr for AptUpgradeEvent {
    type Err = ();

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        if input.starts_with("Progress: [") {
            let (_, progress) = input.split_at(11);
            if progress.len() == 5 {
                if let Ok(percent) = progress[..progress.len() - 2].trim_left().parse::<u8>() {
                    return Ok(AptUpgradeEvent::Progress { percent });
                }
            }
        } else if input.starts_with("Processing triggers for ") {
            let (_, input) = input.split_at(24);
            if let Some(package) = input.split_whitespace().next() {
                return Ok(AptUpgradeEvent::Processing { package: package.to_owned() });
            }
        } else if input.starts_with("Setting up ") {
            let (_, input) = input.split_at(11);
            if let Some(package) = input.split_whitespace().next() {
                return Ok(AptUpgradeEvent::SettingUp { package: package.to_owned() });
            }
        } else if input.starts_with("Unpacking ") {
            let (_, input) = input.split_at(10);
            let mut fields = input.split_whitespace();
            if let (Some(package), Some(version), Some(over)) =
                (fields.next(), fields.next(), fields.nth(1))
            {
                if version.len() > 2 && over.len() > 2 {
                    return Ok(AptUpgradeEvent::Unpacking {
                        package: package.to_owned(),
                        version: version[1..version.len() - 1].to_owned(),
                        over: over[1..over.len() - 1].to_owned(),
                    });
                }
            }
        }

        Err(())
    }
}
