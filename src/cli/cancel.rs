use clap::Clap;
use pop_upgrade::client::{Client, Error as ClientError};

/// cancels any process which is currently in progress
#[derive(Debug, Clap)]
pub struct Cancel {}

impl Cancel {
    pub fn run(&self) -> Result<(), ClientError> { Client::new()?.cancel() }
}
