use std::io::{self, Write};

use apt_cmd::AptUpgradeEvent;
use clap::Parser;
use pop_upgrade::{
    client::{Client, Continue, ReleaseInfo, Signal},
    misc,
    release::UpgradeMethod,
};

use crate::app::{color, prompt};

use super::util::{log_result, write_apt_event};

const FETCH_RESULT_STR: &str = "Package fetch status";
const FETCH_RESULT_SUCCESS: &str = "cargo has been loaded successfully";
const FETCH_RESULT_ERROR: &str = "package-fetching aborted";

const UPGRADE_RESULT_STR: &str = "Release upgrade status";
const UPGRADE_RESULT_SUCCESS: &str = "systems are go for launch: reboot now";
const UPGRADE_RESULT_ERROR: &str = "release upgrade aborted";

/// update the system, and fetch the packages for the next release
#[derive(Parser)]
pub struct Upgrade {
    /// Attempt to upgrade to the next release, even if it is not released
    #[clap(short, long)]
    force_next: bool,
}

impl Upgrade {
    pub fn run(&self, client: &Client) -> anyhow::Result<()> {
        let method = UpgradeMethod::Offline;
        let forcing = self.force_next || pop_upgrade::development_releases_enabled();
        let ReleaseInfo { current, next, build, .. } = client.release_check(forcing)?;

        if atty::is(atty::Stream::Stdout) {
            let mut buffer = String::new();
            pintln!(
                (color::primary("Current Release")) ": " (color::secondary(&current)) "\n"
                (color::primary("Upgrading to")) ": " (color::secondary(&next)) "\n"
                (color::primary("New version available")) ": " (color::secondary(misc::format_build_number(build, &mut buffer)))
            );
        }

        // Only upgrade if an upgrade is possible, or if being forced to upgrade.
        if forcing || build >= 0 {
            // Ask to perform the release upgrade, and then listen for its signals.
            client.release_upgrade(method, current.as_ref(), next.as_ref())?;
            let mut recall = event_listen_release_upgrade(client)?;

            // Repeat as necessary.
            while recall {
                println!(
                    "{}: {}",
                    color::primary("Event"),
                    color::secondary("attempting to perform upgrade again")
                );
                client.release_upgrade(method, current.as_ref(), next.as_ref())?;
                recall = event_listen_release_upgrade(client)?;
            }

            // Finalize the release upgrade.
            client.release_upgrade_finalize()?;
        } else {
            println!("no release available to upgrade to");
        }
        Ok(())
    }
}

fn event_listen_release_upgrade(client: &Client) -> anyhow::Result<bool> {
    let mut reset = false;
    let recall = &mut false;

    let result = client.event_listen(
        Client::release_upgrade_status,
        |new_status| {
            log_result(
                new_status.status,
                UPGRADE_RESULT_STR,
                UPGRADE_RESULT_SUCCESS,
                UPGRADE_RESULT_ERROR,
                &new_status.why,
            );
        },
        |_client, signal| {
            match signal {
                Signal::PackageFetchResult(status) => {
                    log_result(
                        status.status,
                        FETCH_RESULT_STR,
                        FETCH_RESULT_SUCCESS,
                        FETCH_RESULT_ERROR,
                        &status.why,
                    );
                }
                Signal::PackageFetched(package) => {
                    println!(
                        "{} ({}/{}): {}",
                        color::primary("Fetched"),
                        color::info(package.completed),
                        color::info(package.total),
                        color::secondary(&package.package)
                    );
                }
                Signal::PackageFetching(package) => {
                    println!("{} {}", color::primary("Fetching"), color::secondary(&package));
                }
                Signal::PackageUpgrade(event) => {
                    match AptUpgradeEvent::from_dbus_map(event.clone().into_iter()) {
                        Ok(event) => write_apt_event(event),
                        Err(()) => error!("failed to unpack the upgrade event: {:?}", event),
                    }
                }
                Signal::RecoveryDownloadProgress(progress) => {
                    print!(
                        "\r{} {}/{} {}",
                        color::primary("Fetched"),
                        color::info(progress.progress / 1024),
                        color::info(progress.total / 1024),
                        color::primary("MiB")
                    );

                    let _ = io::stdout().flush();

                    reset = true;
                }
                Signal::RecoveryEvent(event) => {
                    if reset {
                        reset = false;
                        println!();
                    }

                    println!(
                        "{}: {}",
                        color::primary("Recovery event"),
                        <&'static str>::from(event)
                    );
                }
                Signal::ReleaseResult(status) => {
                    if !*recall {
                        log_result(
                            status.status,
                            UPGRADE_RESULT_STR,
                            UPGRADE_RESULT_SUCCESS,
                            UPGRADE_RESULT_ERROR,
                            &status.why,
                        );
                    }

                    return Ok(Continue(false));
                }
                Signal::ReleaseEvent(event) => {
                    println!(
                        "{}: {}",
                        color::primary("Event"),
                        color::secondary(<&'static str>::from(event))
                    );
                }
                Signal::NoConnection => {
                    println!(
                        "{}",
                        color::error(
                            "Failed to connect to an apt repository. You may not be connected to \
                             the Internet."
                        )
                    );

                    let prompt = format!("    {} y/N", color::primary("Try again?"));

                    if prompt::get_bool(&prompt, false) {
                        *recall = true;
                    } else {
                        return Ok(Continue(false));
                    }
                }
                Signal::RecoveryResult(_) => (),
            }

            Ok(Continue(true))
        },
    );

    if !*recall {
        result?;
    }

    Ok(*recall)
}
