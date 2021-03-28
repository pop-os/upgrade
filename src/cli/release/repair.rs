use clap::Clap;
use pop_upgrade::client::{Client, Error as ClientError};

/// search for issues in the system, and repair them
#[derive(Debug, Clap)]
pub struct Repair {
    /// Attempt to upgrade to the next release, even if it is not released
    #[clap(short, long)]
    force_next: bool,
}

impl Repair {
    pub fn run(&self, client: &Client) -> Result<(), ClientError> { client.release_repair() }
}
