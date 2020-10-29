#[macro_use]
extern crate fomat_macros;

#[macro_use]
extern crate log;

#[macro_use]
extern crate shrinkwraprs;

#[macro_use]
extern crate thiserror;

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

use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use std::{error::Error as _, process::exit};

use self::error::{Error, InitError};

pub fn main() {
    let _ = setup_logging(::log::LevelFilter::Debug);

    let clap = App::new("pop-upgrade")
        .about("Pop!_OS Upgrade Utility")
        .global_setting(AppSettings::ColoredHelp)
        .global_setting(AppSettings::UnifiedHelpMessage)
        .setting(AppSettings::SubcommandRequiredElseHelp)
        // Recovery partition tools.
        .subcommand(
            SubCommand::with_name("cancel")
                .about("cancels any process which is currently in progress"),
        )
        .subcommand(
            SubCommand::with_name("daemon")
                .about("launch a daemon for integration with control centers like GNOME's"),
        )
        .subcommand(
            SubCommand::with_name("recovery")
                .about("tools for managing the recovery partition")
                .setting(AppSettings::SubcommandRequiredElseHelp)
                // Reboot into the recovery partition.
                .subcommand(
                    SubCommand::with_name("default-boot")
                        .about("set the recovery partition as the default boot target")
                        .arg(
                            Arg::with_name("reboot")
                                .help("immediately reboot the system into the recovery partition")
                                .long("reboot"),
                        ),
                )
                // Upgrade the recovery partition.
                .subcommand(
                    SubCommand::with_name("upgrade")
                        .about("upgrade the recovery partition")
                        .setting(AppSettings::SubcommandRequiredElseHelp)
                        .subcommand(
                            SubCommand::with_name("from-release")
                                .about("update the recovery partition using a the Pop release API")
                                .arg(
                                    Arg::with_name("VERSION")
                                        .help("release version to fetch. IE: `18.04`"),
                                )
                                .arg(
                                    Arg::with_name("ARCH")
                                        .help("release arch to fetch: IE: `nvidia` or `intel`"),
                                )
                                .arg(
                                    Arg::with_name("next")
                                        .help(
                                            "fetches the next release's ISO if VERSION is not set",
                                        )
                                        .long("next"),
                                ),
                        )
                        // .subcommand(
                        //     SubCommand::with_name("from-file")
                        //         .about("update the recovery partition using an ISO on the system")
                        //         .arg(
                        //             Arg::with_name("PATH")
                        //                 .help("location to fetch the from file")
                        //                 .required(true),
                        //         ),
                        // ),
                ),
        )
        // Distribution release tools
        .subcommand(
            SubCommand::with_name("release")
                .about("check for new distribution releases, or upgrade to a new release")
                .setting(AppSettings::SubcommandRequiredElseHelp)
                .subcommand(
                    SubCommand::with_name("check").about("check for a new distribution release"),
                )
                .subcommand(
                    SubCommand::with_name("dismiss")
                        .about("dismiss the current release notification (LTS only)"),
                )
                .subcommand(
                    SubCommand::with_name("update")
                        .about("fetch the latest updates for the current release")
                        .arg(
                            Arg::with_name("download-only")
                                .help(
                                    "instruct the daemon to fetch updates, without installing them",
                                )
                                .short("d")
                                .long("download-only"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("refresh")
                        .about("refresh the existing OS (requires recovery partition)")
                        .arg(Arg::with_name("disable"))
                        .arg(Arg::with_name("enable")),
                )
                .subcommand(
                    SubCommand::with_name("repair")
                        .about("search for issues in the system, and repair them"),
                )
                .subcommand(
                    SubCommand::with_name("upgrade")
                        .about("update the system, and fetch the packages for the next release")
                        .arg(
                            Arg::with_name("force-next")
                                .help(
                                    "Attempt to upgrade to the next release, even if it is not \
                                     released",
                                )
                                .short("f")
                                .long("force-next")
                                .global(true),
                        ),
                ),
        )
        .subcommand(
            SubCommand::with_name("status").about("get the status of the pop upgrade daemon"),
        );

    if let Err(why) = main_(&clap.get_matches()) {
        eprintln!("pop-upgrade: {}", why);

        let mut source = why.source();
        while let Some(why) = source {
            eprintln!("  caused by: {}", why);
            source = why.source();
        }

        exit(1);
    }
}

fn main_(matches: &ArgMatches) -> Result<(), Error> {
    init()?;

    match matches.subcommand() {
        ("cancel", _) => Client::new()?.cancel()?,
        ("daemon", _) => Daemon::init()?,
        (other, Some(matches)) => {
            let client = Client::new()?;
            let func = match other {
                "recovery" => Client::recovery,
                "release" => Client::release,
                "status" => Client::status,
                _ => unreachable!(),
            };

            func(&client, matches)?
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
