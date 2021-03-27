use crate::Error;
use structopt::StructOpt;

/// launch a daemon for integration with control centers like GNOME's
#[derive(Debug, StructOpt)]
pub struct Daemon {}

impl Daemon {
    pub fn run(&self) -> Result<(), Error> {
        pop_upgrade::daemon::Daemon::init()?;
        Ok(())
    }
}
