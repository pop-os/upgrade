use clap::Clap;
use pop_upgrade::{
    client::{Client, Error as ClientError},
    release::RefreshOp,
};

/// refresh the existing OS (requires recovery partition)
#[derive(Debug, Clap)]
pub struct Command {
    #[clap(arg_enum)]
    action: Option<Action>,
}

#[derive(Debug, Clap)]
pub enum Action {
    Enable,
    Disable,
}

impl Command {
    pub fn run(&self, client: &Client) -> Result<(), ClientError> {
        match self.action {
            Some(Action::Enable) => client.refresh_os(RefreshOp::Enable)?,
            Some(Action::Disable) => client.refresh_os(RefreshOp::Disable)?,
            None => client.refresh_os(RefreshOp::Status)?,
        };

        println!("reboot to boot into the recovery partition to begin the refresh install");

        Ok(())
    }
}
