use crate::{notify::notify, Error};
use chrono::{offset::TimeZone, Utc};
use pop_upgrade::{
    client::{Client, Error as ClientError, ReleaseInfo},
    daemon::{DismissEvent, DISMISSED, INSTALL_DATE},
    misc,
    release::eol::{EolDate, EolStatus},
};
use std::{convert::TryFrom, fs, path::Path};
use structopt::StructOpt;
use ubuntu_version::{Codename, Version as UbuntuVersion};

pub fn run(client: &Client) -> Result<(), ClientError> {
    let devel = pop_upgrade::development_releases_enabled();
    let info = client.release_check(devel)?;
    if info.is_lts {
        client.dismiss_notification(DismissEvent::ByUser)?;
    } else {
        println!("Only LTS releases may dismiss notifications");
    }

    Ok(())
}
