use clap::Parser;

mod recovery;
use recovery::Recovery;

mod release;
use release::Release;

#[derive(Parser)]
#[clap(about)]
pub enum App {
    /// cancels any process which is currently in progress
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
}
