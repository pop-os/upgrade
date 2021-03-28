#![deny(clippy::all)]
#![warn(clippy::pedantic)]

mod cli;
mod logging;
mod notify;
use clap::Clap;

use crate::logging::setup_logging;
use pop_upgrade::sighandler;

pub mod error {
    use pop_upgrade::{
        client::Error as ClientError, daemon::DaemonError, recovery::RecoveryError,
        release::ReleaseError,
    };
    use std::io;

    #[derive(Debug, thiserror::Error)]
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

    #[derive(Debug, thiserror::Error)]
    pub enum InitError {
        #[error("failure to create /var/cache/apt/archives/partial directories")]
        AptCacheDirectories(#[source] io::Error),
    }
}

use std::{error::Error as _, process::exit};

use self::error::{Error, InitError};

pub fn main() {
    let _ = setup_logging(::log::LevelFilter::Debug);

    if let Err(why) = main_() {
        eprintln!("pop-upgrade: {}", why);

        let mut source = why.source();
        while let Some(why) = source {
            eprintln!("  caused by: {}", why);
            source = why.source();
        }

        exit(1);
    }
}

fn main_() -> Result<(), Error> {
    let app = cli::Command::parse();
    init()?;
    app.run()
}

fn init() -> Result<(), InitError> {
    sighandler::init();

    ::std::fs::create_dir_all("/var/cache/apt/archives/partial/")
        .map_err(InitError::AptCacheDirectories)
}
