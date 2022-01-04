use clap::Parser;
use pop_upgrade::client::Client;

/// upgrade the recovery partition
#[derive(Parser)]
pub enum Upgrade {
    /// update the recovery partition using a the Pop release API
    FromRelease {
        /// release version to fetch. IE: `18.04`
        version: String,

        /// release arch to fetch: IE: `nvidia` or `intel`
        arch: String,

        /// fetches the next release's ISO if VERSION is not set
        #[clap(long)]
        next: bool,
    },
}

impl Upgrade {
    pub fn run(&self, client: &Client) -> anyhow::Result<()> {
        match self {
            Self::FromRelease { version, arch, next } => {
                let flags = if *next {
                    pop_upgrade::recovery::ReleaseFlags::NEXT
                } else {
                    pop_upgrade::recovery::ReleaseFlags::empty()
                };

                client.recovery_upgrade_release(version, arch, flags)?;
            } // Self::FromFile => THIS DOESN'T EXIST!
        }

        Ok(())
    }
}
