use std::{convert::TryFrom, fs, path::Path};

use chrono::{TimeZone, Utc};
use pop_upgrade::{
    client::{Client, ReleaseInfo},
    daemon, misc,
    release::eol::{EolDate, EolStatus},
};
use ubuntu_version::{Codename, Version as UbuntuVersion};

use crate::notify::notify;

pub fn run(client: &Client) -> anyhow::Result<()> {
    let mut buffer = String::new();
    let ReleaseInfo { current, next, build, is_lts, .. } = client.release_check(false)?;

    if atty::is(atty::Stream::Stdout) {
        println!(
            "      Current Release: {}\n         Next Release: {}\nNew Release available: {}",
            current,
            next,
            misc::format_build_number(build, &mut buffer)
        );
    } else if build >= 0 {
        if is_lts && (dismissed(&next) || dismiss_by_timestamp(client, &next)?) {
            return Ok(());
        }

        let (summary, body) = notification_message(&current, &next);

        let upgrade_panel = if &*current == "18.04" { "info-overview" } else { "upgrade" };

        notify(&summary, &body, || {
            let _ = exec::Command::new("gnome-control-center").arg(upgrade_panel).exec();
        });
    }

    Ok(())
}

/// Check if this release has already been dismissed
fn dismissed(next: &str) -> bool {
    Path::new(daemon::DISMISSED).exists() && {
        fs::read_to_string(daemon::DISMISSED)
            .map(|dismissed| dismissed.as_str() == next)
            .unwrap_or(false)
    }
}

/// Check if the release has been dismissed by timestamp, or can be.
fn dismiss_by_timestamp(client: &Client, next: &str) -> anyhow::Result<bool> {
    if !Path::new(daemon::INSTALL_DATE).exists() && installed_after_release(next) {
        info!("dismissing notification for the latest release automatically");
        let _ = client.dismiss_notification(daemon::DismissEvent::ByTimestamp)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn installed_after_release(next: &str) -> bool {
    match pop_upgrade::install::time() {
        Ok(install_time) => {
            if let Some(pos) = next.find('.') {
                let (major, mut minor) = next.split_at(pos);
                minor = &minor[1..];

                if let (Ok(major), Ok(minor)) = (major.parse::<u8>(), minor.parse::<u8>()) {
                    match Codename::try_from(UbuntuVersion { major, minor, patch: 0 }) {
                        Ok(codename) => return codename.release_timestamp() < install_time as u64,
                        Err(()) => error!("version {} is invalid", next),
                    }
                } else {
                    error!("major ({}) and minor({}) version failed to parse as u8", major, minor);
                }
            } else {
                error!("version {} is invalid", next);
            }
        }
        Err(why) => error!("failed to get install time: {}", why),
    }

    false
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
