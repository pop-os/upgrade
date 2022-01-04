#[macro_use]
extern crate anyhow;

#[macro_use]
extern crate fomat_macros;

#[macro_use]
extern crate log;

#[macro_use]
extern crate thiserror;

mod app;
mod logging;
mod notify;

use crate::logging::setup_logging;
use app::App;
use pop_upgrade::sighandler;

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

use std::{path::Path, process::exit};

use self::error::InitError;

pub fn main() {
    // Service shall not run in a live environment.
    if Path::new("/cdrom/casper/filesystem.squashfs").exists() {
        exit(0);
    }

    let _ = setup_logging(::log::LevelFilter::Debug);

    if let Err(why) = run() {
        eprintln!("pop-upgrade: {}", why);

        let mut source = why.source();
        while let Some(why) = source {
            eprintln!("  caused by: {}", why);
            source = why.source();
        }

        exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    init()?;

    let app = App::from_cli();
    app.run()
}

fn init() -> anyhow::Result<()> {
    sighandler::init();

    ::std::fs::create_dir_all("/var/cache/apt/archives/partial/")
        .map_err(InitError::AptCacheDirectories)?;

    Ok(())
}
