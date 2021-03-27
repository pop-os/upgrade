use crate::{cli::Client, Error};
use structopt::StructOpt;

/// cancels any process which is currently in progress
#[derive(Debug, StructOpt)]
pub struct Cancel {}

impl Cancel {
    pub fn run(&self) -> Result<(), Error> {
        Client::new()?.cancel()?;
        Ok(())
    }
}
