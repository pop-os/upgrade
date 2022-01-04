use clap::Parser;
use pop_upgrade::{
    client::{Client, ReleaseInfo},
    daemon,
    release::RefreshOp,
};

mod check;

mod update;
use update::Update;

mod upgrade;
use upgrade::Upgrade;

mod util;

/// check for new distribution releases, or upgrade to a new release
#[derive(Parser)]
pub enum Release {
    /// check for a new distribution release
    Check,

    /// dismiss the current release notification (LTS only)
    Dismiss,

    /// fetch the latest updates for the current release
    Update(Update),

    /// refresh the existing OS (requires recovery partition)
    #[clap(subcommand)]
    Refresh(Refresh),

    /// search for issues in the system, and repair them
    Repair,

    /// update the system, and fetch the packages for the next release
    Upgrade(Upgrade),
}

impl Release {
    pub fn run(&self, client: &Client) -> anyhow::Result<()> {
        match self {
            Self::Check => check::run(client),
            Self::Dismiss => dismiss(client),
            Self::Update(update) => update.run(client),
            Self::Refresh(action) => {
                client.refresh_os(action.into())?;
                println!("reboot to boot into the recovery partition to begin the refresh install");
                Ok(())
            }
            Self::Repair => Ok(client.release_repair()?),
            Self::Upgrade(upgrade) => upgrade.run(client),
        }
    }
}

#[derive(Parser)]
pub enum Refresh {
    Disable,
    Enable,
}

impl<'a> From<&'a Refresh> for RefreshOp {
    fn from(action: &'a Refresh) -> Self {
        match action {
            Refresh::Disable => Self::Disable,
            Refresh::Enable => Self::Enable,
        }
    }
}

fn dismiss(client: &Client) -> anyhow::Result<()> {
    let devel = pop_upgrade::development_releases_enabled();
    let ReleaseInfo { is_lts, .. } = client.release_check(devel)?;
    if is_lts {
        client.dismiss_notification(daemon::DismissEvent::ByUser)?;
    } else {
        println!("Only LTS releases may dismiss notifications");
    }
    Ok(())
}
