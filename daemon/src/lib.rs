#![deny(clippy::all)]

#[macro_use]
extern crate anyhow;

#[macro_use]
extern crate bitflags;

#[macro_use]
extern crate cascade;

#[macro_use]
extern crate enclose;

#[macro_use]
extern crate fomat_macros;

#[macro_use]
extern crate log;

#[macro_use]
extern crate num_derive;

/// Changelogs for each Pop!_OS release
pub mod changelogs;

/// Validate the SHA256 checksum of a file
pub mod checksum;

/// Features specific to the client for the upgrade daemon
pub mod client;

/// Features specific to the upgrade daemon
pub mod daemon;

/// Functions for determining when the OS was installed
pub mod install;

/// Miscellaneous functions used throughout the library.
pub mod misc;

/// Functions for upgrading the recovery partition
pub mod recovery;

/// Functions for performing release upgrades
pub mod release;

/// Support for interaction with the Pop Release API
pub mod release_api;

/// Function for determinating if the OS requires the NVIDIA or Intel ISO
pub mod release_architecture;

/// Functions for repairing the OS
pub mod repair;

/// Signal-handling capabilities for the daemon.
pub mod sighandler;

/// Determine if the system is in legacy BIOS or EFI mode.
pub mod system_environment;

/// Ubuntu versions
pub mod ubuntu_version;

mod external;
mod fetch;
mod gnome_extensions;

use std::path::Path;

pub static DBUS_NAME: &str = "com.system76.PopUpgrade";
pub static DBUS_PATH: &str = "/com/system76/PopUpgrade";
pub static DBUS_IFACE: &str = "com.system76.PopUpgrade";

pub const DEVELOPMENT_RELEASE_FILE: &str = "/etc/pop-upgrade/devel";

pub const VAR_LIB_DIR: &str = "/var/lib/pop-upgrade";
pub const TRANSITIONAL_SNAPS: &str = "/var/lib/pop-upgrade/transitional_snaps";
pub const RESTART_SCHEDULED: &str = "/var/lib/pop-upgrade/restarting";

pub fn development_releases_enabled() -> bool { Path::new(DEVELOPMENT_RELEASE_FILE).exists() }
