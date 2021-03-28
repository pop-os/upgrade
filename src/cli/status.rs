use crate::Error;
use clap::Clap;

/// get the status of the pop upgrade daemon
#[derive(Debug, Clap)]
pub struct Command {}

impl Command {
    pub fn run(&self) -> Result<(), Error> { unimplemented!() }
}
