use clap::Clap;
use pop_upgrade::daemon::DaemonError;

/// launch a daemon for integration with control centers like GNOME's
#[derive(Debug, Clap)]
pub struct Daemon;

impl Daemon {
    pub fn run(&self) -> Result<(), DaemonError> {
        pop_upgrade::daemon::Daemon::init()?;
        Ok(())
    }
}
