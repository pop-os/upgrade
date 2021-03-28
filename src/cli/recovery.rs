use crate::{cli::color, Error};
use clap::Clap;
use pop_upgrade::{client::Client, daemon::DaemonStatus, recovery::ReleaseFlags};
use std::io::Write;
use yansi::Paint;

const RESULT_STR: &str = "Recovery upgrade status";
const RESULT_SUCCESS: &str = "recovery partition refueled and ready to go";
const RESULT_ERROR: &str = "recovery upgrade aborted";

/// tools for managing the recovery partition
#[derive(Debug, Clap)]
pub enum Recovery {
    /// set the recovery partition as the default boot target
    DefaultBoot {
        /// immediately reboot the system into the recovery partition
        #[clap(long)]
        reboot: bool,
    },

    Upgrade(Upgrade),
}

impl Recovery {
    pub fn run(&self, client: &Client) -> Result<(), Error> {
        match self {
            Self::DefaultBoot { reboot: _ } => unimplemented!(),
            Self::Upgrade(upgrade) => upgrade.run(client)?,
        };

        event_listen_upgrade(client)?;
        Ok(())
    }
}

/// upgrade the recovery partition
#[derive(Debug, Clap)]
pub enum Upgrade {
    FromRelease(FromRelease),
}

impl Upgrade {
    pub fn run(&self, client: &Client) -> Result<(), Error> {
        match self {
            Self::FromRelease(from_release) => from_release.run(client),
        }
    }
}

/// update the recovery partition using a the Pop release API
#[derive(Debug, Clap)]
pub struct FromRelease {
    /// release version to fetch. IE: `18.04`
    #[clap(default_value)]
    version: String,

    /// release arch to fetch: IE: `nvidia` or `intel`
    #[clap(default_value)]
    architecture: String,

    /// fetches the next release's ISO if VERSION is not set
    #[clap(long)]
    next: bool,
}

impl FromRelease {
    pub fn run(&self, client: &Client) -> Result<(), Error> {
        let flags = if self.next { ReleaseFlags::NEXT } else { ReleaseFlags::empty() };

        client.recovery_upgrade_release(&self.version, &self.architecture, flags)?;
        Ok(())
    }
}

fn event_listen_upgrade(client: &Client) -> Result<(), pop_upgrade::client::Error> {
    let mut reset = false;

    client.event_listen(
        DaemonStatus::RecoveryUpgrade,
        Client::recovery_upgrade_release_status,
        |new_status| {
            log_result(new_status.status, RESULT_STR, RESULT_SUCCESS, RESULT_ERROR, &new_status.why)
        },
        move |_client, signal| {
            match signal {
                pop_upgrade::client::Signal::RecoveryDownloadProgress(progress) => {
                    print!(
                        "\r{} {}/{} {}",
                        color::primary("Fetched"),
                        color::info(progress.progress / 1024),
                        color::info(progress.total / 1024),
                        color::primary("MiB")
                    );

                    let _ = std::io::stdout().flush();

                    reset = true;
                }
                pop_upgrade::client::Signal::RecoveryEvent(event) => {
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
                pop_upgrade::client::Signal::RecoveryResult(status) => {
                    if reset {
                        reset = false;
                        println!();
                    }

                    log_result(
                        status.status,
                        RESULT_STR,
                        RESULT_SUCCESS,
                        RESULT_ERROR,
                        &status.why,
                    );

                    return Ok(pop_upgrade::client::Continue(false));
                }
                _ => (),
            }

            Ok(pop_upgrade::client::Continue(true))
        },
    )
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
        color::info(event),
        if status == 0 {
            color::primary(success)
        } else {
            inner = format!("{}: {}", color::error(error), color::error_desc(why));

            Paint::wrapping(inner.as_str())
        }
    );
}
