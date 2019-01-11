extern crate apt_fetcher;
extern crate apt_keyring;
extern crate async_fetcher;
extern crate atomic;
extern crate atty;
extern crate clap;
extern crate disk_types;
extern crate distinst;
#[macro_use]
extern crate err_derive;
extern crate fern;
extern crate futures;
extern crate libc;
#[macro_use]
extern crate log;
extern crate md5;
extern crate os_release;
extern crate parallel_getter;
extern crate promptly;
extern crate reqwest;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate sha2;
extern crate sys_mount;
extern crate sysfs_class;
extern crate systemd_boot_conf;
extern crate tempfile;
extern crate tokio_process;
extern crate tokio;
extern crate yansi;

mod checksum;
mod command;
mod external;
mod logging;
mod misc;
mod recovery;
mod release;
mod release_api;
mod release_architecture;
mod release_version;
mod status;
mod ubuntu_codename;

use crate::logging::setup_logging;
use crate::recovery::recovery;
use crate::release::release;

pub mod error {
    use std::io;
    use super::recovery::RecoveryError;
    use super::release::ReleaseError;

    #[derive(Debug, Error)]
    pub enum Error {
        #[error(display = "recovery subcommand failed: {}", _0)]
        Recovery(RecoveryError),
        #[error(display = "release subcommand failed: {}", _0)]
        Release(ReleaseError),
        #[error(display = "failed to ensure requirements are met: {}", _0)]
        Init(InitError)
    }

    impl From<RecoveryError> for Error {
        fn from(why: RecoveryError) -> Self {
            Error::Recovery(why)
        }
    }

    impl From<ReleaseError> for Error {
        fn from(why: ReleaseError) -> Self {
            Error::Release(why)
        }
    }

    impl From<InitError> for Error {
        fn from(why: InitError) -> Self {
            Error::Init(why)
        }
    }

    #[derive(Debug, Error)]
    pub enum InitError {
        #[error(display = "failure to create /var/cache/apt/archives/partial directories: {}", _0)]
        AptCacheDirectories(io::Error)
    }
}

use clap::{App, AppSettings, Arg, ArgMatches, SubCommand};
use std::process::exit;

use self::error::{Error, InitError};

pub fn main() {
    let _ = setup_logging(::log::LevelFilter::Debug);
    let matches = App::new("pop-upgrade")
        .about("Pop!_OS Upgrade Utility")
        .global_setting(AppSettings::ColoredHelp)
        .global_setting(AppSettings::UnifiedHelpMessage)
        .setting(AppSettings::SubcommandRequiredElseHelp)
        // Recovery partition tools.
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
                                .long("reboot")
                        )
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
                                        .help("release version to fetch. IE: `18.04`")
                                )
                                .arg(
                                    Arg::with_name("ARCH")
                                        .help("release arch to fetch: IE: `nvidia` or `intel`")
                                )
                                .arg(
                                    Arg::with_name("next")
                                        .help("fetches the next release's ISO if VERSION is not set")
                                        .long("next")
                                )
                        )
                        .subcommand(
                            SubCommand::with_name("from-file")
                                .about("update the recovery partition using an ISO on the system")
                                .arg(
                                    Arg::with_name("PATH")
                                        .help("location to fetch the from file")
                                        .required(true)
                                )
                        )
                )
        )
        // Distribution release tools
        .subcommand(
            SubCommand::with_name("release")
                .about("check for new distribution releases, or upgrade to a new release")
                .subcommand(
                    SubCommand::with_name("check")
                        .about("check for a new distribution release")
                )
                .subcommand(
                    SubCommand::with_name("upgrade")
                        .about("update the system, and fetch the packages for the next release")
                        .arg(
                            Arg::with_name("live")
                                .help("forces the system to perform the upgrade live")
                                .long("live")
                        )
                )
        )
        .get_matches();

    if let Err(why) = main_(&matches) {
        eprintln!("pop-upgrade: {}", why);
        exit(1);
    }
}

fn main_(matches: &ArgMatches) -> Result<(), Error> {
    init()?;

    match matches.subcommand() {
        ("recovery", Some(matches)) => recovery(matches)?,
        ("release", Some(matches)) => release(matches)?,
        _ => unreachable!("clap argument parsing failed")
    }

    Ok(())
}

fn init() -> Result<(), InitError> {
    ::std::fs::create_dir_all("/var/cache/apt/archives/partial/")
        .map_err(InitError::AptCacheDirectories)
}
