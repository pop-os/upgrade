use crate::{notify::notify, Error};
use chrono::{offset::TimeZone, Utc};
use pop_upgrade::{
    client::{Client, Error as ClientError, ReleaseInfo},
    daemon::{DismissEvent, DISMISSED, INSTALL_DATE},
    misc,
    release::eol::{EolDate, EolStatus},
};
use std::{convert::TryFrom, fs, path::Path};
use structopt::StructOpt;
use ubuntu_version::{Codename, Version as UbuntuVersion};

mod check;
mod update;
use update::Update;
mod dismiss;

/// check for new distribution releases, or upgrade to a new release
#[derive(Debug, StructOpt)]
pub enum Release {
    /// check for a new distribution release
    Check,

    /// dismiss the current release notification (LTS only)
    Dismiss,

    Update(Update),

    Refresh(Refresh),

    /// search for issues in the system, and repair them
    Repair {
        /// Attempt to upgrade to the next release, even if it is not released
        #[structopt(short, long)]
        force_next: bool,
    },

    /// update the system, and fetch the packages for the next release
    Upgrade,
}

impl Release {
    pub fn run(&self, client: &Client) -> Result<(), Error> {
        match self {
            Self::Check => check::run(client)?,
            Self::Dismiss => dismiss::run(client)?,
            Self::Update(update) => update.run(client)?,
            _ => todo!(),
        };

        Ok(())
    }
}

/// refresh the existing OS (requires recovery partition)
#[derive(Debug, StructOpt)]
pub enum Refresh {
    Enable,
    Disable,
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

fn notification_message(current: &str, next: &str) -> (String, String) {
    match EolDate::fetch() {
        Ok(eol) => match eol.status() {
            EolStatus::Exceeded => {
                return (
                    fomat!("Support for Pop!_OS " (current) " has ended"),
                    fomat!(
                        "Security and application updates are no longer provided for Pop!_OS "
                        (current) ". Upgrade to Pop!_OS " (next) " to keep your computer secure."
                    ),
                );
            }
            EolStatus::Imminent => {
                let (y, m, d) = eol.ymd;
                return (
                    fomat!(
                        "Support for Pop!_OS " (current) " ends "
                        (Utc.ymd(y as i32, m, d).format("%B %-d, %Y"))
                    ),
                    fomat!(
                        "This computer will soon stop receiving updates"
                        ". Upgrade to Pop!_OS " (next) " to keep your computer secure."
                    ),
                );
            }
            EolStatus::Ok => (),
        },
        Err(why) => error!("failed to fetch EOL date: {}", why),
    }

    ("Upgrade Available".into(), fomat!("Pop!_OS " (next) " is available to download"))
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
