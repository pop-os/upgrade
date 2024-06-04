mod colors;
mod prompt;

use self::colors::*;
use crate::notify::notify;
use apt_cmd::AptUpgradeEvent;
use chrono::{TimeZone, Utc};
use clap::ArgMatches;
use num_traits::FromPrimitive;
use pop_upgrade::{
    client,
    daemon::*,
    misc,
    recovery::{RecoveryEvent, ReleaseFlags as RecoveryReleaseFlags},
    release::{
        eol::{EolDate, EolStatus},
        systemd::{self, LoaderEntry},
        RefreshOp, UpgradeEvent, UpgradeMethod,
    },
    ubuntu_version::{Codename, Version as UbuntuVersion},
};
use std::{
    convert::TryFrom,
    fs,
    io::{self, Write},
    path::Path,
};
use yansi::Paint;

const FETCH_RESULT_STR: &str = "Package fetch status";
const FETCH_RESULT_SUCCESS: &str = "cargo has been loaded successfully";
const FETCH_RESULT_ERROR: &str = "package-fetching aborted";

const RECOVERY_RESULT_STR: &str = "Recovery upgrade status";
const RECOVERY_RESULT_SUCCESS: &str = "recovery partition refueled and ready to go";
const RECOVERY_RESULT_ERROR: &str = "recovery upgrade aborted";

const UPGRADE_RESULT_STR: &str = "Release upgrade status";
const UPGRADE_RESULT_SUCCESS: &str = "systems are go for launch: reboot now";
const UPGRADE_RESULT_ERROR: &str = "release upgrade aborted";

#[derive(Shrinkwrap)]
pub struct Client(client::Client);

impl Client {
    pub fn new() -> Result<Self, client::Error> { client::Client::new().map(Client) }

    /// Executes the recovery subcommand of the client.
    pub fn recovery(&self, matches: &ArgMatches) -> anyhow::Result<()> {
        match matches.subcommand() {
            Some(("default-boot", _)) => {
                root_required()?;
                systemd::BootConf::load()?.set_default_boot_variant(&LoaderEntry::Recovery)?;
            }
            Some(("upgrade", matches)) => {
                match matches.subcommand() {
                    Some(("from-release", matches)) => {
                        let version = matches.value_of("VERSION").unwrap_or("");
                        let arch = matches.value_of("ARCH").unwrap_or("");
                        let flags = if matches.is_present("next") {
                            RecoveryReleaseFlags::NEXT
                        } else {
                            RecoveryReleaseFlags::empty()
                        };

                        self.recovery_upgrade_release(version, arch, flags)?;
                    }
                    Some(("from-file", matches)) => {
                        let path = matches.value_of("PATH").expect("missing reqired PATH argument");

                        let _ = self.recovery_upgrade_file(path)?;
                    }
                    _ => unreachable!(),
                }

                self.event_listen_recovery_upgrade()?;
            }
            Some(("check", _)) => {
                let version = self.recovery_version()?;
                pintln!(
                    "version: " (version.version) "\n"
                    "build: " (version.build)
                );
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    pub fn release(&self, matches: &ArgMatches) -> anyhow::Result<()> {
        match matches.subcommand() {
            Some(("dismiss", _)) => {
                let devel = pop_upgrade::development_releases_enabled();
                let (_, _, _, is_lts) = self.release_check(devel)?;
                if is_lts {
                    self.dismiss_notification(DismissEvent::ByUser)?;
                } else {
                    println!("Only LTS releases may dismiss notifications");
                }
            }
            Some(("check", _)) => {
                let (current, next, available, is_lts) = self.release_check(false)?;

                if atty::is(atty::Stream::Stdout) {
                    println!("Checking if {} can be upgraded to {}", current, next);
                } else if available >= 0 {
                    if is_lts && (self.dismissed(&next) || self.dismiss_by_timestamp(&next)?) {
                        return Ok(());
                    }

                    let (summary, body) = notification_message(&current, &next);

                    let upgrade_panel =
                        if &*current == "18.04" { "info-overview" } else { "upgrade" };

                    notify(&summary, &body, || {
                        let _ =
                            exec::Command::new("gnome-control-center").arg(upgrade_panel).exec();
                    });
                }
            }
            // Update the current system, without performing a release upgrade
            Some(("update", matches)) => {
                let updates =
                    self.fetch_updates(Vec::new(), matches.is_present("download-only"))?;

                let client::Fetched { updates_available, completed, total } = updates;

                if !updates_available || total == 0 {
                    println!("no updates available to fetch");
                } else {
                    println!("fetching updates: {} of {} updates fetched", completed, total);
                    self.event_listen_fetch_updates()?;
                }
            }
            // Perform an upgrade to the next release. Supports either systemd or recovery upgrades.
            Some(("upgrade", matches)) => {
                let (method, matches) = (UpgradeMethod::Offline, matches);
                let forcing =
                    matches.is_present("force-next") || pop_upgrade::development_releases_enabled();
                let (current, next, available, _is_lts) = self.release_check(forcing)?;

                if atty::is(atty::Stream::Stdout) {
                    let mut buffer = String::new();
                    pintln!(
                        (color_primary("Current Release")) ": " (color_secondary(&current)) "\n"
                        (color_primary("Upgrading to")) ": " (color_secondary(&next)) "\n"
                        (color_primary("New version available")) ": " (color_secondary(misc::format_build_number(available, &mut buffer)))
                    );
                }

                // Only upgrade if an upgrade is possible, or if being forced to upgrade.
                if forcing || available >= 0 {
                    // Ask to perform the release upgrade, and then listen for its signals.
                    self.release_upgrade(method, current.as_ref(), next.as_ref())?;
                    // Repeat as necessary.

                    while self.event_listen_release_upgrade()? {
                        println!(
                            "{}: {}",
                            color_primary("Event"),
                            color_secondary("attempting to perform upgrade again")
                        );
                        self.release_upgrade(method, current.as_ref(), next.as_ref())?;
                    }

                    // Finalize the release upgrade.
                    self.release_upgrade_finalize()?;
                } else {
                    println!("no release available to upgrade to");
                }
            }
            // Set the recovery partition as the next boot target, and configure it to
            // automatically switch to the refresh view.
            Some(("refresh", matches)) => {
                let action = match matches.subcommand() {
                    Some(("disable", _)) => RefreshOp::Disable,
                    _ => RefreshOp::Enable,
                };

                self.refresh_os(action)?;
                println!("reboot to boot into the recovery partition to begin the refresh install");
            }
            Some(("repair", _)) => {
                self.release_repair()?;
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    pub fn status(&self, _matches: &ArgMatches) -> anyhow::Result<()> {
        let info = self.0.status()?;

        let (status, sub_status) = match DaemonStatus::from_u8(info.status) {
            Some(status) => {
                let x = <&'static str>::from(status);
                let y = match status {
                    DaemonStatus::ReleaseUpgrade => match UpgradeEvent::from_u8(info.sub_status) {
                        Some(sub) => <&'static str>::from(sub),
                        None => "unknown sub_status",
                    },
                    DaemonStatus::RecoveryUpgrade => {
                        match RecoveryEvent::from_u8(info.sub_status) {
                            Some(sub) => <&'static str>::from(sub),
                            None => "unknown sub_status",
                        }
                    }
                    _ => "",
                };

                (x, y)
            }
            None => ("unknown status", ""),
        };

        if sub_status.is_empty() {
            println!("{}", status);
        } else {
            println!("{}: {}", status, sub_status);
        }

        Ok(())
    }

    /// Check if this release has already been dismissed
    fn dismissed(&self, next: &str) -> bool {
        Path::new(DISMISSED).exists() && {
            fs::read_to_string(DISMISSED)
                .map(|dismissed| dismissed.as_str() == next)
                .unwrap_or(false)
        }
    }

    /// Check if the release has been dismissed by timestamp, or can be.
    fn dismiss_by_timestamp(&self, next: &str) -> Result<bool, client::Error> {
        if !Path::new(INSTALL_DATE).exists() && installed_after_release(next) {
            info!("dismissing notification for the latest release automatically");
            let _ = self.dismiss_notification(DismissEvent::ByTimestamp)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn release_check(
        &self,
        force_next: bool,
    ) -> Result<(Box<str>, Box<str>, i16, bool), client::Error> {
        let info = self.0.release_check(force_next)?;

        Ok((info.current, info.next, info.build, info.is_lts))
    }

    fn event_listen_fetch_updates(&self) -> Result<(), client::Error> {
        self.event_listen(
            client::Client::fetch_updates_status,
            |new_status| {
                log_result(
                    new_status.status,
                    FETCH_RESULT_STR,
                    FETCH_RESULT_SUCCESS,
                    FETCH_RESULT_ERROR,
                    &new_status.why,
                );
            },
            |_client, signal| {
                match signal {
                    client::Signal::PackageFetchResult(status) => {
                        log_result(
                            status.status,
                            "Package fetch status",
                            "cargo has been loaded successfully",
                            "package-fetching aborted",
                            &status.why,
                        );

                        return Ok(client::Continue::False);
                    }
                    client::Signal::PackageFetched(status) => {
                        println!(
                            "{} ({}/{}) {}",
                            color_primary("Fetched"),
                            color_info(status.completed),
                            color_info(status.total),
                            color_secondary(status.package)
                        );
                    }
                    client::Signal::PackageUpgrade(event) => {
                        if let Ok(event) = AptUpgradeEvent::from_dbus_map(event.into_iter()) {
                            write_apt_event(event);
                        } else {
                            error!("failed to unpack the upgrade event");
                        }
                    }
                    _ => (),
                }

                Ok(client::Continue::True)
            },
        )
    }

    fn event_listen_recovery_upgrade(&self) -> Result<(), client::Error> {
        let mut reset = false;

        let result = self.event_listen(
            client::Client::recovery_upgrade_release_status,
            |new_status| {
                log_result(
                    new_status.status,
                    RECOVERY_RESULT_STR,
                    RECOVERY_RESULT_SUCCESS,
                    RECOVERY_RESULT_ERROR,
                    &new_status.why,
                );
            },
            |_client, signal| {
                match signal {
                    client::Signal::RecoveryDownloadProgress(progress) => {
                        print!(
                            "\r{} {}/{} {}",
                            color_primary("Fetched"),
                            color_info(progress.progress / 1024),
                            color_info(progress.total / 1024),
                            color_primary("MiB")
                        );

                        let _ = io::stdout().flush();

                        reset = true;
                    }
                    client::Signal::RecoveryEvent(event) => {
                        if reset {
                            reset = false;
                            println!();
                        }

                        println!(
                            "{}: {}",
                            color_primary("Recovery event"),
                            <&'static str>::from(event)
                        );
                    }
                    client::Signal::RecoveryResult(status) => {
                        if reset {
                            reset = false;
                            println!();
                        }

                        log_result(
                            status.status,
                            RECOVERY_RESULT_STR,
                            RECOVERY_RESULT_SUCCESS,
                            RECOVERY_RESULT_ERROR,
                            &status.why,
                        );

                        return Ok(client::Continue::False);
                    }
                    _ => (),
                }

                Ok(client::Continue::True)
            },
        );

        if reset {
            println!();
        }

        result
    }

    fn event_listen_release_upgrade(&self) -> Result<bool, client::Error> {
        let mut reset = false;
        let recall = &mut false;
        let total = &mut 0;

        let result = self.event_listen(
            client::Client::release_upgrade_status,
            |new_status| {
                log_result(
                    new_status.status,
                    UPGRADE_RESULT_STR,
                    UPGRADE_RESULT_SUCCESS,
                    UPGRADE_RESULT_ERROR,
                    &new_status.why,
                );
            },
            |_client, signal| {
                use client::Signal;
                match signal {
                    Signal::PackageFetchResult(status) => {
                        log_result(
                            status.status,
                            FETCH_RESULT_STR,
                            FETCH_RESULT_SUCCESS,
                            FETCH_RESULT_ERROR,
                            &status.why,
                        );
                    }

                    Signal::PackageFetched(package) => {
                        println!(
                            "{} ({}/{}): {}",
                            color_primary("Fetched"),
                            color_info(package.completed),
                            color_info(package.total),
                            color_secondary(&package.package)
                        );
                    }

                    Signal::PackageUpgrade(event) => {
                        match AptUpgradeEvent::from_dbus_map(event.clone().into_iter()) {
                            Ok(event) => write_apt_event(event),
                            Err(()) => error!("failed to unpack the upgrade event: {:?}", event),
                        }
                    }

                    Signal::RecoveryDownloadProgress(progress) => {
                        *total = progress.total / 1024;
                        print!(
                            "\r{} {}/{} {}",
                            color_primary("Fetched"),
                            color_info(progress.progress / 1024),
                            color_info(*total),
                            color_primary("MiB")
                        );

                        let _ = io::stdout().flush();

                        reset = true;
                    }

                    Signal::RecoveryEvent(event) => {
                        if reset {
                            println!(
                                "\r{} {}/{} {}",
                                color_primary("Fetched"),
                                color_info(*total),
                                color_info(*total),
                                color_primary("MiB")
                            );

                            reset = false;
                        }

                        println!(
                            "{}: {}",
                            color_primary("Recovery event"),
                            <&'static str>::from(event)
                        );
                    }

                    Signal::RecoveryResult(status) => {
                        log_result(
                            status.status,
                            RECOVERY_RESULT_STR,
                            RECOVERY_RESULT_SUCCESS,
                            RECOVERY_RESULT_ERROR,
                            &status.why,
                        );
                    }

                    Signal::ReleaseResult(status) => {
                        if !*recall {
                            log_result(
                                status.status,
                                UPGRADE_RESULT_STR,
                                UPGRADE_RESULT_SUCCESS,
                                UPGRADE_RESULT_ERROR,
                                &status.why,
                            );
                        }

                        if status.status != 0 {
                            return Err(client::Error::Status(status.why));
                        }

                        return Ok(client::Continue::False);
                    }

                    Signal::ReleaseEvent(event) => {
                        println!(
                            "{}: {}",
                            color_primary("Event"),
                            color_secondary(<&'static str>::from(event))
                        );
                    }

                    Signal::NoConnection => {
                        println!(
                            "{}",
                            color_error(
                                "Failed to connect to an apt repository. You may not be connected \
                                 to the Internet."
                            )
                        );

                        let prompt = format!("    {} y/N", color_primary("Try again?"));

                        if prompt::get_bool(&prompt, false) {
                            *recall = true;
                        } else {
                            return Ok(client::Continue::False);
                        }
                    }

                    _ => (),
                }

                Ok(client::Continue::True)
            },
        );

        if !*recall {
            result?;
        }

        Ok(*recall)
    }
}

/// If the next release's timestamp is less than the install time.
fn installed_after_release(next: &str) -> bool {
    match pop_upgrade::install::time() {
        Ok(install_time) => {
            if let Some(pos) = next.find('.') {
                let (major, mut minor) = next.split_at(pos);
                minor = &minor[1..];

                if let (Ok(major), Ok(minor)) = (major.parse::<u8>(), minor.parse::<u8>()) {
                    match Codename::try_from(UbuntuVersion { major, minor, patch: 0 }) {
                        Ok(codename) => return codename.release_timestamp() < install_time as u64,
                        Err(()) => error!("version {} is invalid", next),
                    }
                } else {
                    error!("major ({}) and minor({}) version failed to parse as u8", major, minor);
                }
            } else {
                error!("version {} is invalid", next);
            }
        }
        Err(why) => error!("failed to get install time: {}", why),
    }

    false
}

fn notification_message(current: &str, next: &str) -> (String, String) {
    match EolDate::fetch() {
        Ok(eol) => match eol.status() {
            EolStatus::Exceeded => {
                return (
                    fomat!("Support for Pop!_OS " (current) " has ended"),
                    fomat!(
                        "Security and application updates are no longer provided for Pop!_OS "
                        (current) ". Upgrade to Pop!_OS " (next) " to keep your computer secure."
                    ),
                );
            }
            EolStatus::Imminent => {
                let (y, m, d) = eol.ymd;
                return (
                    fomat!(
                        "Support for Pop!_OS " (current) " ends "
                        (Utc.ymd(y as i32, m, d).format("%B %-d, %Y"))
                    ),
                    fomat!(
                        "This computer will soon stop receiving updates"
                        ". Upgrade to Pop!_OS " (next) " to keep your computer secure."
                    ),
                );
            }
            EolStatus::Ok => (),
        },
        Err(why) => error!("failed to fetch EOL date: {}", why),
    }

    ("Upgrade Available".into(), fomat!("Pop!_OS " (next) " is available to download"))
}

fn write_apt_event(event: AptUpgradeEvent) {
    match event {
        AptUpgradeEvent::Processing { package } => {
            println!("{} for {}", color_primary("Processing triggers"), color_secondary(package));
        }
        AptUpgradeEvent::Progress { percent } => {
            println!("{}: {}%", color_primary("Progress"), color_info(percent));
        }
        AptUpgradeEvent::SettingUp { package } => {
            println!("{} {}", color_primary("Setting up"), color_secondary(package));
        }
        AptUpgradeEvent::Unpacking { package, version, over } => {
            println!(
                "{} {} ({}) over ({})",
                color_primary("Unpacking"),
                color_secondary(package),
                color_info(version),
                color_info(over)
            );
        }
        AptUpgradeEvent::WaitingOnLock => {
            println!(
                "{} {}",
                color_primary("Waiting"),
                color_secondary("on a process holding an apt/dpkg lock file")
            );
        }
    }
}

fn log_result(
    status: u8,
    event: &'static str,
    success: &'static str,
    error: &'static str,
    why: &str,
) {
    let inner: String;

    println!(
        "{}: {}",
        color_info(event),
        if status == 0 {
            color_primary(success)
        } else {
            inner = format!("{}: {}", color_error(error), color_error_desc(why));

            inner.as_str().wrap()
        }
    );
}

pub fn root_required() -> anyhow::Result<()> {
    if unsafe { libc::geteuid() == 0 } {
        Ok(())
    } else {
        Err(anyhow!("root is required for this operation"))
    }
}
