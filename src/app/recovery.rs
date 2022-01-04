use clap::Parser;

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
