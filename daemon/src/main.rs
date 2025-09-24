#[macro_use]
extern crate anyhow;

#[macro_use]
extern crate fomat_macros;

#[macro_use]
extern crate log;

#[macro_use]
extern crate shrinkwraprs;

#[macro_use]
extern crate thiserror;

use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

mod cli;
mod logging;
mod notify;

use crate::{cli::Client, logging::setup_logging};
use pop_upgrade::{daemon::Daemon, sighandler};

pub mod error {
    use pop_upgrade::{
        client::Error as ClientError, daemon::DaemonError, recovery::RecoveryError,
        release::ReleaseError,
    };
    use std::io;

    #[derive(Debug, Error)]
    pub enum Error {
        #[error("dbus client error")]
        Client(#[from] ClientError),

        #[error("daemon initialization error")]
        Daemon(#[from] DaemonError),

        #[error("recovery subcommand failed")]
        Recovery(#[from] RecoveryError),

        #[error("release subcommand failed")]
        Release(#[from] ReleaseError),

        #[error("failed to ensure requirements are met")]
        Init(#[from] InitError),
    }

    #[derive(Debug, Error)]
    pub enum InitError {
        #[error("failure to create /var/cache/apt/archives/partial directories")]
        AptCacheDirectories(#[source] io::Error),
    }
}

use std::{path::Path, process::exit, time::Duration};

use self::error::InitError;

#[tokio::main]
async fn main() {
    // Ensure file system caches are synced to prevent recovery ISO download corruption.
    rustix::fs::sync();

    // Fixes a panic in `reqwest::Client::new`
    wait_for_systemd_resolvd().await;

    // Service shall not run in a live environment.
    if Path::new("/cdrom/casper/filesystem.squashfs").exists() {
        exit(0);
    }

    let _ = setup_logging(::log::LevelFilter::Debug);

    let clap = clap::Command::new("pop-upgrade")
        .about("Pop!_OS Upgrade Utility")
        .subcommand_required(true)
        // Recovery partition tools.
        .subcommand(
            clap::Command::new("cancel")
                .about("cancels any process which is currently in progress"),
        )
        .subcommand(
            clap::Command::new("daemon")
                .about("launch a daemon for integration with control centers like GNOME's"),
        )
        .subcommand(
            clap::Command::new("recovery")
                .about("tools for managing the recovery partition")
                .subcommand_required(true)
                // Reboot into the recovery partition.
                .subcommand(
                    clap::Command::new("default-boot")
                        .about("set the recovery partition as the default boot target")
                        .arg(
                            clap::Arg::new("reboot")
                                .help("immediately reboot the system into the recovery partition")
                                .long("reboot")
                                .action(clap::ArgAction::SetTrue),
                        ),
                )
                // Upgrade the recovery partition.
                .subcommand(
                    clap::Command::new("upgrade")
                        .about("upgrade the recovery partition")
                        .subcommand_required(true)
                        .subcommand(
                            clap::Command::new("from-release")
                                .about("update the recovery partition using a the Pop release API")
                                .arg(
                                    clap::Arg::new("VERSION")
                                        .help("release version to fetch. IE: `18.04`"),
                                )
                                .arg(
                                    clap::Arg::new("ARCH")
                                        .help("release arch to fetch: IE: `nvidia` or `intel`"),
                                )
                                .arg(
                                    clap::Arg::new("next")
                                        .help(
                                            "fetches the next release's ISO if VERSION is not set",
                                        )
                                        .long("next")
                                        .action(clap::ArgAction::SetTrue),
                                ),
                        ),
                )
                .subcommand(
                    clap::Command::new("check").about("check the status of the recovery partition"),
                ),
        )
        // Distribution release tools
        .subcommand(
            clap::Command::new("release")
                .about("check for new distribution releases, or upgrade to a new release")
                .subcommand_required(true)
                .subcommand(
                    clap::Command::new("check").about("check for a new distribution release"),
                )
                .subcommand(
                    clap::Command::new("dismiss")
                        .about("dismiss the current release notification (LTS only)"),
                )
                .subcommand(
                    clap::Command::new("update")
                        .about("fetch the latest updates for the current release")
                        .arg(
                            clap::Arg::new("download-only")
                                .help(
                                    "instruct the daemon to fetch updates, without installing them",
                                )
                                .short('d')
                                .long("download-only")
                                .action(clap::ArgAction::SetTrue),
                        ),
                )
                .subcommand(
                    clap::Command::new("refresh")
                        .about("refresh the existing OS (requires recovery partition)")
                        .subcommand(clap::Command::new("disable"))
                        .subcommand(clap::Command::new("enable")),
                )
                .subcommand(
                    clap::Command::new("repair")
                        .about("search for issues in the system, and repair them"),
                )
                .subcommand(
                    clap::Command::new("upgrade")
                        .about("update the system, and fetch the packages for the next release")
                        .arg(
                            clap::Arg::new("force-next")
                                .help(
                                    "Attempt to upgrade to the next release, even if it is not \
                                     released",
                                )
                                .short('f')
                                .long("force-next")
                                .action(clap::ArgAction::SetTrue)
                                .global(true),
                        ),
                ),
        )
        .subcommand(clap::Command::new("status").about("get the status of the pop upgrade daemon"));

    if main_(&clap.get_matches()).await.is_err() {
        exit(1);
    }
}

async fn main_(matches: &clap::ArgMatches) -> anyhow::Result<()> {
    init()?;

    match matches.subcommand() {
        Some(("cancel", _)) => Client::new()?.cancel()?,
        Some(("daemon", _)) => Daemon::init().await?,
        Some((other, matches)) => {
            let mut client = Client::new()?;

            if std::env::var_os("S76_TEST").is_none() {
                println!("checking if pop-upgrade requires an update");
                if client.update_and_restart()? {
                    println!("waiting for daemon to update and restart");

                    let file = std::path::Path::new(pop_upgrade::RESTART_SCHEDULED);
                    while file.exists() {
                        if crate::sighandler::status().is_some() {
                            std::process::exit(1);
                        }

                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }

                    std::thread::sleep(std::time::Duration::from_secs(1));

                    println!("reconnecting to pop-upgrade daemon");
                    client = Client::new()?;
                }
            }

            let func = match other {
                "recovery" => Client::recovery,
                "release" => Client::release,
                "status" => Client::status,
                _ => unreachable!(),
            };

            func(&client, matches)?;
        }
        _ => unreachable!("clap argument parsing failed"),
    }

    Ok(())
}

fn init() -> Result<(), InitError> {
    sighandler::init();

    ::std::fs::create_dir_all("/var/cache/apt/archives/partial/")
        .map_err(InitError::AptCacheDirectories)
}

/// Ensure that the systemd DNS resolv file is generated before proceeding.
async fn wait_for_systemd_resolvd() {
    let resolv = Path::new("/etc/resolv.conf");

    while !resolv.exists() {
        info!("waiting for resolv.conf to be generated");
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
