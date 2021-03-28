use crate::cli::{
    color,
    util::{log_result, write_apt_event},
};
use apt_cmd::AptUpgradeEvent;
use clap::Clap;
use pop_upgrade::{
    client::{Client, Error as ClientError, Signal},
    daemon::DaemonStatus,
    misc,
};
use std::{
    io,
    io::{BufRead, Write},
};

const UPGRADE_RESULT_STR: &str = "Release upgrade status";
const UPGRADE_RESULT_SUCCESS: &str = "systems are go for launch: reboot now";
const UPGRADE_RESULT_ERROR: &str = "release upgrade aborted";

const FETCH_RESULT_STR: &str = "Package fetch status";
const FETCH_RESULT_SUCCESS: &str = "cargo has been loaded successfully";
const FETCH_RESULT_ERROR: &str = "package-fetching aborted";

#[derive(Debug, Clap)]
pub struct Upgrade {
    /// Attempt to upgrade to the next release, even if it is not \
    // released
    #[clap(long)]
    force_next: bool,
}

impl Upgrade {
    pub fn run(&self, client: &Client) -> Result<(), ClientError> {
        let forcing = self.force_next || pop_upgrade::development_releases_enabled();
        let info = client.release_check(forcing)?;

        if atty::is(atty::Stream::Stdout) {
            let mut buffer = String::new();
            pintln!(
                (color::primary("Current Release")) ": " (color::secondary(&info.current)) "\n"
                (color::primary("Upgrading to")) ": " (color::secondary(&info.next)) "\n"
                (color::primary("New version available")) ": " (color::secondary(misc::format_build_number(info.build, &mut buffer)))
            );
        }

        if forcing || info.build >= 0 {
            client.release_upgrade(
                pop_upgrade::release::UpgradeMethod::Offline,
                &info.current,
                &info.next,
            )?;
            let mut recall = event_listen_upgrade(client)?;

            // Repeat as necessary.
            while recall {
                println!(
                    "{}: {}",
                    color::primary("Event"),
                    color::secondary("attempting to perform upgrade again")
                );
                client.release_upgrade(
                    pop_upgrade::release::UpgradeMethod::Offline,
                    &info.current,
                    &info.next,
                )?;
                recall = event_listen_upgrade(client)?;
            }

            // Finalize the release upgrade.
            client.release_upgrade_finalize()?;
        } else {
            println!("no release available to upgrade to");
        }

        Ok(())
    }
}

fn event_listen_upgrade(client: &Client) -> Result<bool, ClientError> {
    let recall = &mut false;

    let result = client.event_listen(
        DaemonStatus::ReleaseUpgrade,
        Client::release_upgrade_status,
        |new_status| {
            log_result(
                new_status.status,
                UPGRADE_RESULT_STR,
                UPGRADE_RESULT_SUCCESS,
                UPGRADE_RESULT_ERROR,
                &new_status.why,
            )
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

                    return Ok(pop_upgrade::client::Continue(false));
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

                    if prompt_message(&prompt, false) {
                        *recall = true;
                    } else {
                        return Ok(pop_upgrade::client::Continue(false));
                    }
                }
                _ => (),
            }

            Ok(pop_upgrade::client::Continue(true))
        },
    );

    if !*recall {
        result?;
    }

    Ok(*recall)
}

// Write a prompt to the terminal, and wait for an answer.
fn prompt_message(message: &str, default: bool) -> bool {
    let stdin = io::stdin();
    let mut stdin = stdin.lock();

    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    let answer = &mut String::with_capacity(16);

    enum Answer {
        Continue,
        Break(bool),
    }

    let mut display_prompt = move || -> io::Result<Answer> {
        answer.clear();

        stdout.write_all(message.as_bytes())?;
        stdout.flush()?;

        stdin.read_line(answer)?;

        if answer.is_empty() {
            return Ok(Answer::Break(default));
        } else if answer.starts_with('y') || answer.starts_with('Y') || answer == "true" {
            return Ok(Answer::Break(true));
        } else if answer.starts_with('n') || answer.starts_with('N') || answer == "false" {
            return Ok(Answer::Break(false));
        }

        stdout.write_all(b"The answer must be either `y` or `n`.\n")?;
        Ok(Answer::Continue)
    };

    loop {
        match display_prompt() {
            Ok(Answer::Continue) => continue,
            Ok(Answer::Break(answer)) => break answer,
            Err(_why) => break default,
        }
    }
}
