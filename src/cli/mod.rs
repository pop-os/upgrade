use crate::Error;
use clap::{AppSettings, Clap};
use pop_upgrade::{client::Client, daemon::Daemon};

mod color;
mod recovery;
mod release;
mod status;
mod util;

/// Pop!_OS Upgrade Utility
#[derive(Debug, Clap)]
#[clap(global_setting(AppSettings::ColoredHelp))]
pub enum Command {
    /// cancels any process which is currently in progress
    Cancel,
    /// launch a daemon for integration with control centers like GNOME's
    Daemon,
    Recovery(recovery::Command),
    Release(release::Command),
    Status(status::Command),
}

impl Command {
    pub fn run(&self) -> Result<(), Error> {
        match self {
            Self::Cancel => Client::new()?.cancel()?,
            Self::Daemon => Daemon::init()?,
            Self::Recovery(command) => {
                let client = update_and_restart()?;
                command.run(&client)?;
            }
            Self::Release(command) => {
                let client = update_and_restart()?;
                command.run(&client)?;
            }
            Self::Status(command) => {
                update_and_restart()?;
                command.run()?;
            }
        };

        Ok(())
    }
}

fn update_and_restart() -> Result<Client, Error> {
    let mut client = Client::new()?;

    println!("checking if pop-upgrade requires an update");
    if client.update_and_restart()? {
        println!("waiting for daemon to update and restart");

        let file = std::path::Path::new(pop_upgrade::RESTART_SCHEDULED);
        while file.exists() {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }

        std::thread::sleep(std::time::Duration::from_secs(1));

        println!("reconnecting to pop-upgrade daemon");
        client = Client::new()?;
    }
    Ok(client)
}
