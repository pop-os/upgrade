use crate::cli::{
    color,
    release::{FETCH_RESULT_ERROR, FETCH_RESULT_STR, FETCH_RESULT_SUCCESS},
    util::{log_result, write_apt_event},
};
use apt_cmd::AptUpgradeEvent;
use clap::Clap;
use pop_upgrade::{
    client::{Client, Error as ClientError},
    daemon::DaemonStatus,
};

/// fetch the latest updates for the current release
#[derive(Debug, Clap)]
pub struct Command {
    /// instruct the daemon to fetch updates, without installing them
    #[clap(short, long)]
    download_only: bool,
}

impl Command {
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
                        color::primary("Fetched"),
                        color::info(status.completed),
                        color::info(status.total),
                        color::secondary(status.package)
                    );
                }
                pop_upgrade::client::Signal::PackageFetching(package) => {
                    println!("{} {}", color::primary("Fetching"), color::secondary(package));
                }
                pop_upgrade::client::Signal::PackageUpgrade(event) => {
                    if let Ok(event) = AptUpgradeEvent::from_dbus_map(event.into_iter()) {
                        write_apt_event(event);
                    } else {
                        log::error!("failed to unpack the upgrade event");
                    }
                }
                _ => (),
            }

            Ok(pop_upgrade::client::Continue(true))
        },
    )
}
