use clap::Parser;
use pop_upgrade::{client::Client, release::systemd};

mod upgrade;
use upgrade::Upgrade;

/// tools for managing the recovery partition
#[derive(Parser)]
pub enum Recovery {
    /// set the recovery partition as the default boot target
    DefaultBoot {
        /// immediately reboot the system into the recovery partition
        #[clap(long)]
        reboot: bool,
    },

    #[clap(subcommand)]
    Upgrade(Upgrade),

    /// check the status of the recovery partition
    Check,
}

impl Recovery {
    pub fn run(&self, client: &Client) -> anyhow::Result<()> {
        match self {
            Self::DefaultBoot { reboot: _ } => {
                root_required()?;
                systemd::BootConf::load()?
                    .set_default_boot_variant(&systemd::LoaderEntry::Recovery)?;
            }
            Self::Upgrade(upgrade) => upgrade.run(client)?,
            Self::Check => {
                let version = client.recovery_version()?;
                pintln!(
                    "version: " (version.version) "\n"
                    "build: " (version.build)
                );
            }
        }

        Ok(())
    }
}

fn root_required() -> anyhow::Result<()> {
    if unsafe { libc::geteuid() == 0 } {
        Ok(())
    } else {
        Err(anyhow!("root is required for this operation"))
    }
}
