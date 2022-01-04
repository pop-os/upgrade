use clap::Parser;
use pop_upgrade::{client::Client, daemon::Daemon};

mod recovery;
use recovery::Recovery;

mod release;
use release::Release;

mod color;
mod prompt;

mod status;

#[derive(Parser)]
#[clap(about)]
pub enum App {
    Cancel,

    /// launch a daemon for integration with control centers like GNOME's
    Daemon,

    #[clap(subcommand)]
    Recovery(Recovery),

    #[clap(subcommand)]
    Release(Release),

    /// get the status of the pop upgrade daemon
    Status,
}

impl App {
    pub fn from_cli() -> Self { Self::parse() }

    pub fn run(&self) -> anyhow::Result<()> {
        match self {
            App::Cancel => Ok(Client::new()?.cancel()?),
            App::Daemon => Ok(Daemon::init()?),
            App::Recovery(recovery) => {
                let client = init_client()?;
                recovery.run(&client)
            }
            App::Release(release) => {
                let client = init_client()?;
                release.run(&client)
            }
            App::Status => {
                let client = init_client()?;
                status::run(&client)
            }
        }
    }
}

fn init_client() -> anyhow::Result<Client> {
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
