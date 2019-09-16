#[macro_use]
extern crate err_derive;
#[macro_use]
extern crate log;
#[macro_use]
extern crate shrinkwraprs;

mod cli;
mod logging;

use crate::{cli::Client, logging::setup_logging};
use pop_upgrade::daemon::Daemon;

pub mod error {
    use pop_upgrade::{
        client::Error as ClientError, daemon::DaemonError, recovery::RecoveryError,
        release::ReleaseError,
    };
    use std::io;

    #[derive(Debug, Error)]
    pub enum Error {
        #[error(display = "dbus client error: {}", _0)]
        Client(ClientError),
        #[error(display = "daemon initialization error: {}", _0)]
        Daemon(DaemonError),
        #[error(display = "recovery subcommand failed: {}", _0)]
        Recovery(RecoveryError),
        #[error(display = "release subcommand failed: {}", _0)]
        Release(ReleaseError),
        #[error(display = "failed to ensure requirements are met: {}", _0)]
        Init(InitError),
    }

    impl From<ClientError> for Error {
        fn from(why: ClientError) -> Self { Error::Client(why) }
    }

    impl From<DaemonError> for Error {
        fn from(why: DaemonError) -> Self { Error::Daemon(why) }
    }

    impl From<RecoveryError> for Error {
        fn from(why: RecoveryError) -> Self { Error::Recovery(why) }
    }

    impl From<ReleaseError> for Error {
        fn from(why: ReleaseError) -> Self { Error::Release(why) }
    }

    impl From<InitError> for Error {
        fn from(why: InitError) -> Self { Error::Init(why) }
    }

    #[derive(Debug, Error)]
    pub enum InitError {
        #[error(display = "failure to create /var/cache/apt/archives/partial directories: {}", _0)]
        AptCacheDirectories(io::Error),
    }
}

use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use std::process::exit;

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
                        .setting(AppSettings::SubcommandRequiredElseHelp)
                        .arg(
                            Arg::with_name("force-next")
                                .help(
                                    "Attempt to upgrade to the next release, even if it is not \
                                     released",
                                )
                                .short("f")
                                .long("force-next")
                                .global(true),
                        )
                        .subcommand(SubCommand::with_name("systemd").about(
                            "apply system upgrades offline with systemd's offline-update service",
                        ))
                        .subcommand(SubCommand::with_name("recovery").about(
                            "utilize the recovery partition for performing an offline update",
                        )),
                ),
        )
        .subcommand(
            SubCommand::with_name("status").about("get the status of the pop upgrade daemon"),
        );

    if let Err(why) = main_(&clap.get_matches()) {
        eprintln!("pop-upgrade: {}", why);
        exit(1);
    }
}

fn main_(matches: &ArgMatches) -> Result<(), Error> {
    init()?;

    match matches.subcommand() {
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
    ::std::fs::create_dir_all("/var/cache/apt/archives/partial/")
        .map_err(InitError::AptCacheDirectories)
}
