use apt_cmd::AptUpgradeEvent;
use clap::Parser;
use pop_upgrade::client::{Client, Continue, Signal};

use crate::app::color;

use super::util::{log_result, write_apt_event};

const FETCH_RESULT_STR: &str = "Package fetch status";
const FETCH_RESULT_SUCCESS: &str = "cargo has been loaded successfully";
const FETCH_RESULT_ERROR: &str = "package-fetching aborted";

/// fetch the latest updates for the current release
#[derive(Parser)]
pub struct Update {
    /// instruct the daemon to fetch updates, without installing them
    #[clap(long, short)]
    download_only: bool,
}

impl Update {
    pub fn run(&self, client: &Client) -> anyhow::Result<()> {
        let pop_upgrade::client::Fetched { updates_available, completed, total } =
            client.fetch_updates(Vec::new(), self.download_only)?;

        if !updates_available || total == 0 {
            println!("no updates available to fetch");
        } else {
            println!("fetching updates: {} of {} updates fetched", completed, total);
            event_listen_fetch_updates(client)?;
        }
        Ok(())
    }
}

fn event_listen_fetch_updates(client: &Client) -> anyhow::Result<()> {
    client.event_listen(
        Client::fetch_updates_status,
        |new_status| {
            log_result(
                new_status.status,
                FETCH_RESULT_STR,
                FETCH_RESULT_SUCCESS,
                FETCH_RESULT_ERROR,
                &new_status.why,
            );
        },
        |_client, signal| {
            match signal {
                Signal::PackageFetchResult(status) => {
                    log_result(
                        status.status,
                        "Package fetch status",
                        "cargo has been loaded successfully",
                        "package-fetching aborted",
                        &status.why,
                    );

                    return Ok(Continue(false));
                }
                Signal::PackageFetched(status) => {
                    println!(
                        "{} ({}/{}) {}",
                        color::primary("Fetched"),
                        color::info(status.completed),
                        color::info(status.total),
                        color::secondary(status.package)
                    );
                }
                Signal::PackageFetching(package) => {
                    println!("{} {}", color::primary("Fetching"), color::secondary(package));
                }
                Signal::PackageUpgrade(event) => {
                    if let Ok(event) = AptUpgradeEvent::from_dbus_map(event.into_iter()) {
                        write_apt_event(event);
                    } else {
                        error!("failed to unpack the upgrade event");
                    }
                }
                _ => (),
            }

            Ok(Continue(true))
        },
    )?;
    Ok(())
}
