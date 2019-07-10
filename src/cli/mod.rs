mod colors;

use self::colors::*;

use apt_cli_wrappers::AptUpgradeEvent;
use clap::ArgMatches;
use dbus::{self, Message};
use num_traits::FromPrimitive;
use pop_upgrade::{
    client,
    daemon::*,
    misc,
    recovery::{RecoveryEvent, ReleaseFlags as RecoveryReleaseFlags},
    release::{RefreshOp, UpgradeEvent, UpgradeMethod},
};
use promptly::Promptable;
use std::io::{self, Write};
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
    pub fn new() -> Result<Self, client::Error> {
        client::Client::new().map(Client)
    }

    /// Executes the recovery subcommand of the client.
    pub fn recovery(&self, matches: &ArgMatches) -> Result<(), client::Error> {
        match matches.subcommand() {
            ("upgrade", Some(matches)) => {
                match matches.subcommand() {
                    ("from-release", Some(matches)) => {
                        let version = matches.value_of("VERSION").unwrap_or("");
                        let arch = matches.value_of("ARCH").unwrap_or("");
                        let flags = if matches.is_present("next") {
                            RecoveryReleaseFlags::NEXT
                        } else {
                            RecoveryReleaseFlags::empty()
                        };

                        let _ = self.recovery_upgrade_release(version, arch, flags)?;
                    }
                    ("from-file", Some(matches)) => {
                        let path = matches
                            .value_of("PATH")
                            .expect("missing reqired PATH argument");

                        let _ = self.recovery_upgrade_file(path)?;
                    }
                    _ => unreachable!(),
                }

                self.event_listen_recovery_upgrade()?;
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    pub fn release(&self, matches: &ArgMatches) -> Result<(), client::Error> {
        match matches.subcommand() {
            ("check", _) => {
                let mut buffer = String::new();
                let (current, next, available) = self.release_check()?;

                println!(
                    "      Current Release: {}\n         Next Release: {}\nNew Release Available: \
                     {}",
                    current,
                    next,
                    misc::format_build_number(available, &mut buffer)
                );
            }
            // Update the current system, without performing a release upgrade
            ("update", Some(matches)) => {
                let updates =
                    self.fetch_updates(Vec::new(), matches.is_present("download-only"))?;

                let client::Fetched {
                    updates_available,
                    completed,
                    total,
                } = updates;

                eprintln!("{} {} {}", updates_available, completed, total);
                if !updates_available || total == 0 {
                    println!("no updates available to fetch");
                } else {
                    println!(
                        "fetching updates: {} of {} updates fetched",
                        completed, total
                    );
                    self.event_listen_fetch_updates()?;
                }
            }
            // Perform an upgrade to the next release. Supports either systemd or recovery upgrades.
            ("upgrade", Some(matches)) => {
                let (method, matches) = match matches.subcommand() {
                    ("systemd", Some(matches)) => (UpgradeMethod::Offline, matches),
                    ("recovery", Some(matches)) => (UpgradeMethod::Recovery, matches),
                    _ => unreachable!(),
                };

                let (current, next, available) = self.release_check()?;

                // Only upgrade if an upgrade is possible, or if being forced to upgrade.
                if matches.is_present("force-next") || available.is_some() {
                    // Before doing a release upgrade with the recovery partition, ensure that
                    // the recovery partition has been updated in advance.
                    if let UpgradeMethod::Recovery = method {
                        self.recovery_upgrade_release("", "", RecoveryReleaseFlags::empty())?;
                        self.event_listen_recovery_upgrade()?;
                    }

                    // Ask to perform the release upgrade, and then listen for its signals.
                    self.release_upgrade(method, current.as_ref(), next.as_ref())?;
                    let mut recall = self.event_listen_release_upgrade()?;

                    // Repeat as necessary.
                    while recall {
                        info!("attempting to perform upgrade again");
                        self.release_upgrade(method, current.as_ref(), next.as_ref())?;
                        recall = self.event_listen_release_upgrade()?;
                    }
                } else {
                    println!("no release available to upgrade to");
                }
            }
            // Set the recovery partition as the next boot target, and configure it to
            // automatically switch to the refresh view.
            ("refresh", Some(matches)) => {
                let action = match () {
                    _ if matches.is_present("enable") => RefreshOp::Enable,
                    _ if matches.is_present("disable") => RefreshOp::Disable,
                    _ => RefreshOp::Status,
                };

                self.refresh_os(action)?;
                println!("reboot to boot into the recovery partition to begin the refresh install");
            }
            ("repair", Some(_)) => {
                self.release_repair()?;
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    pub fn status(&self, _matches: &ArgMatches) -> Result<(), client::Error> {
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

    fn release_check<'a>(&self) -> Result<(Box<str>, Box<str>, Option<u16>), client::Error> {
        let info = self.0.release_check()?;

        let build = if info.build < 0 {
            None
        } else {
            Some(info.build as u16)
        };

        Ok((info.current, info.next, build))
    }

    fn event_listen_fetch_updates(&self) -> Result<(), client::Error> {
        self.event_listen(
            DaemonStatus::FetchingPackages,
            client::Client::fetch_updates_status,
            |new_status| {
                log_result(
                    new_status.status,
                    FETCH_RESULT_STR,
                    FETCH_RESULT_SUCCESS,
                    FETCH_RESULT_ERROR,
                    &new_status.why,
                )
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

                        return Ok(client::Continue(false));
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
                    client::Signal::PackageFetching(package) => {
                        println!("{} {}", color_primary("Fetching"), color_secondary(package));
                    }
                    client::Signal::PackageUpgrade(event) => {
                        if let Ok(event) = AptUpgradeEvent::from_dbus_map(event.into_iter()) {
                            write_apt_event(event);
                        } else {
                            eprintln!("failed to unpack the upgrade event");
                        }
                    }
                    _ => (),
                }

                Ok(client::Continue(true))
            },
        )
    }

    fn event_listen_recovery_upgrade(&self) -> Result<(), client::Error> {
        let mut reset = false;

        self.event_listen(
            DaemonStatus::RecoveryUpgrade,
            client::Client::recovery_upgrade_release_status,
            |new_status| {
                log_result(
                    new_status.status,
                    RECOVERY_RESULT_STR,
                    RECOVERY_RESULT_SUCCESS,
                    RECOVERY_RESULT_ERROR,
                    &new_status.why,
                )
            },
            move |_client, signal| {
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

                        return Ok(client::Continue(false));
                    }
                    _ => (),
                }

                Ok(client::Continue(true))
            },
        )
    }

    fn event_listen_release_upgrade(&self) -> Result<bool, client::Error> {
        let recall = &mut false;

        self.event_listen(
            DaemonStatus::ReleaseUpgrade,
            client::Client::release_upgrade_status,
            |new_status| {
                log_result(
                    new_status.status,
                    UPGRADE_RESULT_STR,
                    UPGRADE_RESULT_SUCCESS,
                    UPGRADE_RESULT_ERROR,
                    &new_status.why,
                )
            },
            |client, signal| {
                match signal {
                    client::Signal::PackageFetchResult(status) => {
                        log_result(
                            status.status,
                            FETCH_RESULT_STR,
                            FETCH_RESULT_SUCCESS,
                            FETCH_RESULT_ERROR,
                            &status.why,
                        );
                    }
                    client::Signal::PackageFetched(package) => {
                        println!(
                            "{} ({}/{}): {}",
                            color_primary("Fetched"),
                            color_info(package.completed),
                            color_info(package.total),
                            color_secondary(&package.package)
                        );
                    }
                    client::Signal::PackageFetching(package) => {
                        println!(
                            "{} {}",
                            color_primary("Fetching"),
                            color_secondary(&package)
                        );
                    }
                    client::Signal::PackageUpgrade(event) => {
                        if let Ok(event) = AptUpgradeEvent::from_dbus_map(event.into_iter()) {
                            write_apt_event(event);
                        } else {
                            eprintln!("failed to unpack the upgrade event");
                        }
                    }
                    client::Signal::ReleaseResult(status) => {
                        log_result(
                            status.status,
                            UPGRADE_RESULT_STR,
                            UPGRADE_RESULT_SUCCESS,
                            UPGRADE_RESULT_ERROR,
                            &status.why,
                        );

                        return Ok(client::Continue(false));
                    }
                    client::Signal::ReleaseEvent(event) => {
                        println!(
                            "{}: {}",
                            color_primary("Release Event"),
                            <&'static str>::from(event)
                        );
                    }
                    client::Signal::RepoCompatError(err) => {
                        let client::RepoCompatError { success, failure } = err;
                        println!("{}:", color_error("Incompatible repositories detected"));

                        for (url, why) in &failure {
                            println!(
                                "    {}: {}:\n        {}",
                                color_error("Error"),
                                color_tertiary(url),
                                color_error_desc(why),
                            );
                        }

                        for url in success {
                            println!("    {}: {}", color_primary("Success"), color_tertiary(url));
                        }

                        println!("{}", color_primary("Requesting user input:"));

                        let repos = failure.iter().map(|(url, _)| url).map(|url| {
                            let prompt = format!(
                                "    {}: ({})? y/N",
                                color_secondary("Keep repository"),
                                color_tertiary(url)
                            );

                            (url, <Option<bool>>::prompt(prompt).unwrap_or(false))
                        });

                        info!("sending message");
                        client.repo_modify(repos)?;
                        info!("message sent");

                        *recall = true;
                    }
                    _ => (),
                }

                Ok(client::Continue(true))
            },
        )?;

        Ok(*recall)
    }
}

fn write_apt_event(event: AptUpgradeEvent) {
    let dpkg = color_primary("Dpkg");
    match event {
        AptUpgradeEvent::Processing { package } => {
            println!(
                "{}: {} for {}",
                dpkg,
                color_secondary("Processing triggers"),
                color_info(package)
            );
        }
        AptUpgradeEvent::Progress { percent } => {
            println!(
                "{}: {}: {}%",
                dpkg,
                color_secondary("Progress"),
                color_info(percent)
            );
        }
        AptUpgradeEvent::SettingUp { package } => {
            println!(
                "{}: {} {}",
                dpkg,
                color_secondary("Setting up"),
                color_tertiary(package)
            );
        }
        AptUpgradeEvent::Unpacking {
            package,
            version,
            over,
        } => {
            println!(
                "{}: {} {} ({}) over ({})",
                dpkg,
                color_secondary("Unpacking"),
                color_tertiary(package),
                color_info(version),
                color_info(over)
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

            Paint::wrapping(inner.as_str())
        }
    );
}
