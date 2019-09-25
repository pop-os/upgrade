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

pub static DBUS_NAME: &str = "com.system76.PopUpgrade";
pub static DBUS_PATH: &str = "/com/system76/PopUpgrade";
pub static DBUS_IFACE: &str = "com.system76.PopUpgrade";
