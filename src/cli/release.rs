use crate::Error;
use clap::Clap;
use pop_upgrade::{
    client::{Client, Error as ClientError},
    daemon::{DismissEvent, DISMISSED, INSTALL_DATE},
};
use std::{convert::TryFrom, fs, path::Path};
use ubuntu_version::{Codename, Version as UbuntuVersion};

mod check;
mod dismiss;
mod refresh;
mod repair;
mod update;
mod upgrade;

const FETCH_RESULT_STR: &str = "Package fetch status";
const FETCH_RESULT_SUCCESS: &str = "cargo has been loaded successfully";
const FETCH_RESULT_ERROR: &str = "package-fetching aborted";

/// check for new distribution releases, or upgrade to a new release
#[derive(Debug, Clap)]
pub enum Command {
    /// check for a new distribution release
    Check,

    /// dismiss the current release notification (LTS only)
    Dismiss,

    Update(update::Command),
    Refresh(refresh::Command),
    Repair(repair::Command),
    Upgrade(upgrade::Command),
}

impl Command {
    pub fn run(&self, client: &Client) -> Result<(), Error> {
        match self {
            Self::Check => check::run(client)?,
            Self::Dismiss => dismiss::run(client)?,
            Self::Update(command) => command.run(client)?,
            Self::Refresh(command) => command.run(client)?,
            Self::Repair(command) => command.run(client)?,
            Self::Upgrade(command) => command.run(client)?,
        };

        Ok(())
    }
}

/// Check if the release has been dismissed by timestamp, or can be.
fn dismiss_by_timestamp(client: &Client, next: &str) -> Result<bool, ClientError> {
    if !Path::new(INSTALL_DATE).exists() && installed_after_release(next) {
        info!("dismissing notification for the latest release automatically");
        let _ = client.dismiss_notification(DismissEvent::ByTimestamp)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// If the next release's timestamp is less than the install time.
fn installed_after_release(next: &str) -> bool {
    match pop_upgrade::install::time() {
        Ok(install_time) => match next.find('.') {
            Some(pos) => {
                let (major, mut minor) = next.split_at(pos);
                minor = &minor[1..];

                match (major.parse::<u8>(), minor.parse::<u8>()) {
                    (Ok(major), Ok(minor)) => {
                        match Codename::try_from(UbuntuVersion { major, minor, patch: 0 }) {
                            Ok(codename) => {
                                return codename.release_timestamp() < install_time as u64
                            }
                            Err(()) => error!("version {} is invalid", next),
                        }
                    }
                    _ => error!(
                        "major ({}) and minor({}) version failed to parse as u8",
                        major, minor
                    ),
                }
            }
            None => error!("version {} is invalid", next),
        },
        Err(why) => error!("failed to get install time: {}", why),
    }

    false
}

/// Check if this release has already been dismissed
fn dismissed(next: &str) -> bool {
    Path::new(DISMISSED).exists() && {
        fs::read_to_string(DISMISSED).map(|dismissed| dismissed.as_str() == next).unwrap_or(false)
    }
}
