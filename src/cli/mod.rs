use crate::Error;
use clap::{AppSettings, Clap};
use pop_upgrade::client::Client;

mod cancel;
mod color;
mod daemon;
mod recovery;
mod release;
mod status;
mod util;

/// Pop!_OS Upgrade Utility
#[derive(Debug, Clap)]
#[clap(global_setting(AppSettings::ColoredHelp))]
pub enum Command {
    Cancel(cancel::Command),
    Daemon(daemon::Command),
    Recovery(recovery::Command),
    Release(release::Command),
    Status(status::Command),
}

impl Command {
    pub fn run(&self) -> Result<(), Error> {
        match self {
            Self::Cancel(cancel) => cancel.run()?,
            Self::Daemon(daemon) => daemon.run()?,
            Self::Recovery(recovery) => {
                let client = update_and_restart()?;
                recovery.run(&client)?;
            }
            Self::Release(release) => {
                let client = update_and_restart()?;
                release.run(&client)?;
            }
            Self::Status(status) => {
                update_and_restart()?;
                status.run()?;
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
