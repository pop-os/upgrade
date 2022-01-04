use clap::Parser;

/// check for new distribution releases, or upgrade to a new release
#[derive(Parser)]
pub enum Release {
    /// check for a new distribution release
    Check,

    /// dismiss the current release notification (LTS only)
    Dismiss,

    /// fetch the latest updates for the current release
    Update {
        /// instruct the daemon to fetch updates, without installing them
        #[clap(long, short)]
        download_only: bool,
    },

    /// refresh the existing OS (requires recovery partition)
    #[clap(subcommand)]
    Refresh(Refresh),

    /// search for issues in the system, and repair them
    Repair,

    /// update the system, and fetch the packages for the next release
    Upgrade {
        /// Attempt to upgrade to the next release, even if it is not released
        #[clap(short, long)]
        force_next: bool,
    },
}

#[derive(Parser)]
pub enum Refresh {
    Disable,
    Enable,
}
