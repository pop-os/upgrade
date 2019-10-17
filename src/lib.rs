#![deny(clippy::all)]
#![allow(clippy::new_ret_no_self)]
#![allow(clippy::useless_attribute)]

#[macro_use]
extern crate bitflags;
#[macro_use]
extern crate err_derive;
#[macro_use]
extern crate log;
#[macro_use]
extern crate num_derive;

pub mod changelogs;
pub mod checksum;
pub mod client;
pub mod daemon;
mod external;
pub mod misc;
pub mod recovery;
pub mod release;
pub mod release_api;
pub mod release_architecture;
pub mod repair;
pub mod repos;
pub mod system_environment;

use std::path::Path;

pub static DBUS_NAME: &str = "com.system76.PopUpgrade";
pub static DBUS_PATH: &str = "/com/system76/PopUpgrade";
pub static DBUS_IFACE: &str = "com.system76.PopUpgrade";

pub const DEVELOPMENT_RELEASE_FILE: &str = "/etc/pop-upgrade/devel";

pub const VAR_LIB_DIR: &str = "/var/lib/pop-upgrade";
pub const TRANSITIONAL_SNAPS: &str = "/var/lib/pop-upgrade/transitional_snaps";

pub fn development_releases_enabled() -> bool { Path::new(DEVELOPMENT_RELEASE_FILE).exists() }
