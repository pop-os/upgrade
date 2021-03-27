use crate::{cli::colors::*, notify::notify, Error};
use apt_cmd::AptUpgradeEvent;
use chrono::{offset::TimeZone, Utc};
use pop_upgrade::{
    client::{Client, Error as ClientError, ReleaseInfo},
    daemon::{DaemonStatus, DismissEvent, DISMISSED, INSTALL_DATE},
    misc,
    release::eol::{EolDate, EolStatus},
};
use std::{convert::TryFrom, fs, path::Path};
use structopt::StructOpt;
use ubuntu_version::{Codename, Version as UbuntuVersion};
use yansi::Paint;

const FETCH_RESULT_STR: &str = "Package fetch status";
const FETCH_RESULT_SUCCESS: &str = "cargo has been loaded successfully";
const FETCH_RESULT_ERROR: &str = "package-fetching aborted";

/// fetch the latest updates for the current release
#[derive(Debug, StructOpt)]
pub struct Update {
    /// instruct the daemon to fetch updates, without installing them
    #[structopt(short, long)]
    download_only: bool,
}

impl Update {
    pub fn run(&self, client: &Client) -> Result<(), ClientError> {
        let updates = client.fetch_updates(Vec::new(), self.download_only)?;

        if !updates.updates_available || updates.total == 0 {
            println!("no updates available to fetch");
        } else {
            println!(
                "fetching updates: {} of {} updates fetched",
                updates.completed, updates.total
            );
            event_listen_fetch_updates(client)?;
        }
        Ok(())
    }
}

fn event_listen_fetch_updates(client: &Client) -> Result<(), ClientError> {
    client.event_listen(
        DaemonStatus::FetchingPackages,
        Client::fetch_updates_status,
        |new_status| {
            log_result(
                new_status.status,
                FETCH_RESULT_STR,
                FETCH_RESULT_SUCCESS,
                FETCH_RESULT_ERROR,
                &new_status.why,
            )
        },
        |_client, signal| {
            match signal {
                pop_upgrade::client::Signal::PackageFetchResult(status) => {
                    log_result(
                        status.status,
                        "Package fetch status",
                        "cargo has been loaded successfully",
                        "package-fetching aborted",
                        &status.why,
                    );

                    return Ok(pop_upgrade::client::Continue(false));
                }
                pop_upgrade::client::Signal::PackageFetched(status) => {
                    println!(
                        "{} ({}/{}) {}",
                        color_primary("Fetched"),
                        color_info(status.completed),
                        color_info(status.total),
                        color_secondary(status.package)
                    );
                }
                pop_upgrade::client::Signal::PackageFetching(package) => {
                    println!("{} {}", color_primary("Fetching"), color_secondary(package));
                }
                pop_upgrade::client::Signal::PackageUpgrade(event) => {
                    if let Ok(event) = AptUpgradeEvent::from_dbus_map(event.into_iter()) {
                        write_apt_event(event);
                    } else {
                        error!("failed to unpack the upgrade event");
                    }
                }
                _ => (),
            }

            Ok(pop_upgrade::client::Continue(true))
        },
    )
}

fn write_apt_event(event: AptUpgradeEvent) {
    match event {
        AptUpgradeEvent::Processing { package } => {
            println!("{} for {}", color_primary("Processing triggers"), color_secondary(package));
        }
        AptUpgradeEvent::Progress { percent } => {
            println!("{}: {}%", color_primary("Progress"), color_info(percent));
        }
        AptUpgradeEvent::SettingUp { package } => {
            println!("{} {}", color_primary("Setting up"), color_secondary(package));
        }
        AptUpgradeEvent::Unpacking { package, version, over } => {
            println!(
                "{} {} ({}) over ({})",
                color_primary("Unpacking"),
                color_secondary(package),
                color_info(version),
                color_info(over)
            );
        }
        AptUpgradeEvent::WaitingOnLock => {
            println!(
                "{} {}",
                color_primary("Waiting"),
                color_secondary("on a process holding an apt/dpkg lock file")
            );
        }
    }
}

fn log_result(
    status: u8,
    event: &'static str,
    success: &'static str,
    error: &'static str,
    why: &str,
) {
    let inner: String;

    println!(
        "{}: {}",
        color_info(event),
        if status == 0 {
            color_primary(success)
        } else {
            inner = format!("{}: {}", color_error(error), color_error_desc(why));

            Paint::wrapping(inner.as_str())
        }
    );
}
