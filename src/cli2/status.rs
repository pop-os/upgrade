use crate::Error;
use structopt::StructOpt;

/// get the status of the pop upgrade daemon
#[derive(Debug, StructOpt)]
pub struct Status {}

impl Status {
    pub fn run(&self) -> Result<(), Error> { unimplemented!() }
}
