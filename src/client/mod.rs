use crate::{
    daemon::*,
    misc,
    recovery::{RecoveryEvent, ReleaseFlags as RecoveryReleaseFlags},
    release::{UpgradeEvent, UpgradeMethod},
    DBUS_IFACE, DBUS_NAME, DBUS_PATH,
};
use apt_cli_wrappers::AptUpgradeEvent;
use clap::ArgMatches;
use dbus::{
    self, BusType, Connection, ConnectionItem, Message, MessageItem, MessageItemArray, Signature,
};
use num_traits::FromPrimitive;
use promptly::Promptable;
use std::{
    collections::HashMap,
    io::{self, Write},
    iter,
};
use yansi::Paint;

const TIMEOUT: i32 = 3000;

const FETCH_RESULT_STR: &str = "Package fetch status";
const FETCH_RESULT_SUCCESS: &str = "cargo has been loaded successfully";
const FETCH_RESULT_ERROR: &str = "package-fetching aborted";

const RECOVERY_RESULT_STR: &str = "Recovery upgrade status";
const RECOVERY_RESULT_SUCCESS: &str = "recovery partition refueled and ready to go";
const RECOVERY_RESULT_ERROR: &str = "recovery upgrade aborted";

const UPGRADE_RESULT_STR: &str = "Release upgrade status";
const UPGRADE_RESULT_SUCCESS: &str = "systems are go for launch: reboot now";
const UPGRADE_RESULT_ERROR: &str = "release upgrade aborted";

struct Continue(pub bool);

#[derive(Debug, Error)]
pub enum ClientError {
    #[error(display = "failed to add match on client connection: {}", _0)]
    AddMatch(dbus::Error),
    #[error(display = "failed to create a dbus connection to the system bus: {}", _0)]
    Connection(dbus::Error),
    #[error(display = "dbus service returned an error message: {}", _0)]
    ErrorResponse(dbus::Error),
    #[error(display = "failed to create new method call for {}: {}", method, why)]
    BadCall { method: &'static str, why: String },
    #[error(display = "dbus service responded with a type mismatch: {}", _0)]
    BadResponse(dbus::arg::TypeMismatchError),
    #[error(display = "daemon status integer was outside the acceptable range of values")]
    DaemonStatusOutOfRange,
}

pub struct Client {
    bus: Connection,
}

impl Client {
    pub fn new() -> Result<Self, ClientError> {
        fn add_match(cbus: &Connection, member: &'static str) -> Result<(), ClientError> {
            cbus.add_match(&format!("interface='{}',member='{}'", DBUS_IFACE, member))
                .map_err(ClientError::AddMatch)?;

            Ok(())
        }

        Connection::get_private(BusType::System).map_err(ClientError::Connection).and_then(|bus| {
            {
                let bus = &bus;
                add_match(bus, signals::PACKAGE_FETCH_RESULT)?;
                add_match(bus, signals::PACKAGE_FETCHED)?;
                add_match(bus, signals::PACKAGE_FETCHING)?;
                add_match(bus, signals::PACKAGE_UPGRADE)?;
                add_match(bus, signals::RECOVERY_DOWNLOAD_PROGRESS)?;
                add_match(bus, signals::RECOVERY_RESULT)?;
                add_match(bus, signals::RECOVERY_EVENT)?;
                add_match(bus, signals::RELEASE_RESULT)?;
                add_match(bus, signals::RELEASE_EVENT)?;
                add_match(bus, signals::REPO_COMPAT_ERROR)?;
            }

            Ok(Client { bus })
        })
    }

    fn recovery_by_release(
        &self,
        version: &str,
        arch: &str,
        flags: RecoveryReleaseFlags,
    ) -> Result<Message, ClientError> {
        let flags: u8 = flags.bits();
        let args: Vec<MessageItem> = vec![version.into(), arch.into(), flags.into()];

        self.call_method(methods::RECOVERY_UPGRADE_RELEASE, args.into_iter())
    }

    /// Executes the recovery subcommand of the client.
    pub fn recovery(&self, matches: &ArgMatches) -> Result<(), ClientError> {
        match matches.subcommand() {
            ("upgrade", Some(matches)) => {
                let _ = match matches.subcommand() {
                    ("from-release", Some(matches)) => {
                        let version = matches.value_of("VERSION").unwrap_or("");
                        let arch = matches.value_of("ARCH").unwrap_or("");
                        let flags = if matches.is_present("next") {
                            RecoveryReleaseFlags::NEXT
                        } else {
                            RecoveryReleaseFlags::empty()
                        };

                        self.recovery_by_release(version, arch, flags)
                    }
                    ("from-file", Some(matches)) => {
                        let path = matches.value_of("PATH").expect("missing reqired PATH argument");

                        self.call_method(methods::RECOVERY_UPGRADE_FILE, iter::once(path.into()))
                    }
                    _ => unreachable!(),
                }?;

                self.event_listen_recovery_upgrade()?;
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    pub fn release(&self, matches: &ArgMatches) -> Result<(), ClientError> {
        match matches.subcommand() {
            ("check", _) => {
                let mut message = None;
                let mut buffer = String::new();
                let (current, next, available) = self.release_check(&mut message)?;

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
                let packages = MessageItemArray::new(
                    Vec::<String>::new().into_iter().map(MessageItem::from).collect(),
                    Signature::from_slice(b"as\0").unwrap(),
                )
                .unwrap();

                let args = iter::once(MessageItem::Array(packages))
                    .chain(iter::once(matches.is_present("download-only").into()));
                let message = self.call_method(methods::FETCH_UPDATES, args)?;
                let (fetching, completed, total) =
                    message.read3::<bool, u32, u32>().map_err(ClientError::BadResponse)?;

                eprintln!("{} {} {}", fetching, completed, total);
                if !fetching || total == 0 {
                    println!("no updates available to fetch");
                } else {
                    println!("fetching updates: {} of {} updates fetched", completed, total);
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

                let mut message = None;
                let (current, next, available) = self.release_check(&mut message)?;

                // Only upgrade if an upgrade is possible, or if being forced to upgrade.
                if matches.is_present("force-next") || available.is_some() {
                    // Before doing a release upgrade with the recovery partition, ensure that
                    // the recovery partition has been updated in advance.
                    if let UpgradeMethod::Recovery = method {
                        self.recovery_by_release("", "", RecoveryReleaseFlags::empty())?;
                        self.event_listen_recovery_upgrade()?;
                    }

                    // Ask to perform the release upgrade, and then listen for its signals.
                    let args = vec![(method as u8).into(), current.into(), next.into()];
                    let _message = self.call_method(methods::RELEASE_UPGRADE, args.into_iter())?;
                    let mut recall = self.event_listen_release_upgrade()?;

                    // Repeat as necessary.
                    while recall {
                        info!("attempting to perform upgrade again");
                        let args = vec![(method as u8).into(), current.into(), next.into()];
                        let _message = self.call_method(methods::RELEASE_UPGRADE, args.into_iter())?;
                        recall = self.event_listen_release_upgrade()?;
                    }
                } else {
                    println!("no release available to upgrade to");
                }
            }
            // Set the recovery partition as the next boot target, and configure it to
            // automatically switch to the refresh view.
            ("refresh", Some(_)) => {
                let _ = self.call_method(methods::REFRESH_OS, iter::empty())?;
                println!("reboot to boot into the recovery partition to begin the refresh install");
            }
            ("repair", Some(_)) => {
                let _message = self.call_method(methods::RELEASE_REPAIR, iter::empty())?;
            }
            _ => unreachable!(),
        }

        Ok(())
    }

    pub fn status(&self, _matches: &ArgMatches) -> Result<(), ClientError> {
        let message = self.call_method(methods::STATUS, iter::empty())?;
        let (status, sub_status) = message.read2::<u8, u8>().map_err(ClientError::BadResponse)?;

        let (status, sub_status) = match DaemonStatus::from_u8(status) {
            Some(status) => {
                let x = <&'static str>::from(status);
                let y = match status {
                    DaemonStatus::ReleaseUpgrade => match UpgradeEvent::from_u8(sub_status) {
                        Some(sub) => <&'static str>::from(sub),
                        None => "unknown sub_status",
                    },
                    DaemonStatus::RecoveryUpgrade => match RecoveryEvent::from_u8(sub_status) {
                        Some(sub) => <&'static str>::from(sub),
                        None => "unknown sub_status",
                    },
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

    fn release_check<'a>(
        &self,
        message: &'a mut Option<Message>,
    ) -> Result<(&'a str, &'a str, Option<u16>), ClientError> {
        *message = Some(self.call_method(methods::RELEASE_CHECK, iter::empty())?);
        let (c, n, a) = message
            .as_mut()
            .unwrap()
            .read3::<&str, &str, i16>()
            .map_err(ClientError::BadResponse)?;

        let a = if a < 0 { None } else { Some(a as u16) };
        Ok((c, n, a))
    }

    fn call_method<A: Iterator<Item = MessageItem>>(
        &self,
        method: &'static str,
        args: A,
    ) -> Result<Message, ClientError> {
        let mut m = Message::new_method_call(DBUS_NAME, DBUS_PATH, DBUS_IFACE, method)
            .map_err(|why| ClientError::BadCall { method, why })?;

        for arg in args {
            m = m.append(arg);
        }

        self.bus.send_with_reply_and_block(m, TIMEOUT).map_err(ClientError::ErrorResponse)
    }

    fn event_listen<F>(
        &self,
        expected_status: DaemonStatus,
        status_name: &'static str,
        event_name: &'static str,
        success: &'static str,
        error: &'static str,
        mut event: F,
    ) -> Result<(), ClientError>
    where
        F: FnMut(&Self, Message) -> Result<Continue, ClientError>,
    {
        for item in self.bus.iter(3000) {
            if let ConnectionItem::Nothing = item {
                if !self.status_is(expected_status)? {
                    let message = self.call_method(status_name, iter::empty())?;
                    let (status, why) =
                        message.read2::<u8, &str>().map_err(ClientError::BadResponse)?;
                    log_result(status, event_name, success, error, why);

                    break;
                }
            } else if let Some(signal) = filter_signal(item) {
                if !event(self, signal)?.0 {
                    break;
                }
            }
        }

        Ok(())
    }

    fn event_listen_fetch_updates(&self) -> Result<(), ClientError> {
        self.event_listen(
            DaemonStatus::FetchingPackages,
            methods::FETCH_UPDATES_STATUS,
            FETCH_RESULT_STR,
            FETCH_RESULT_SUCCESS,
            FETCH_RESULT_ERROR,
            |_client, signal| {
                match &*signal.member().unwrap() {
                    signals::PACKAGE_FETCH_RESULT => {
                        let (status, why) =
                            signal.read2::<u8, &str>().map_err(ClientError::BadResponse)?;

                        log_result(
                            status,
                            "Package fetch status",
                            "cargo has been loaded successfully",
                            "package-fetching aborted",
                            why,
                        );

                        return Ok(Continue(false));
                    }
                    signals::PACKAGE_FETCHED => {
                        let (name, completed, total) =
                            signal.read3::<&str, u32, u32>().map_err(ClientError::BadResponse)?;

                        println!(
                            "{} ({}/{}) {}",
                            Paint::green("Fetched").bold(),
                            Paint::yellow(completed).bold(),
                            Paint::yellow(total).bold(),
                            Paint::magenta(name).bold()
                        );
                    }
                    signals::PACKAGE_FETCHING => {
                        let name = signal.read1::<&str>().map_err(ClientError::BadResponse)?;

                        println!(
                            "{} {}",
                            Paint::green("Fetching").bold(),
                            Paint::magenta(name).bold()
                        );
                    }
                    signals::PACKAGE_UPGRADE => {
                        let event = signal
                            .read1::<HashMap<&str, String>>()
                            .map_err(ClientError::BadResponse)?;

                        if let Ok(event) = AptUpgradeEvent::from_dbus_map(event) {
                            write_apt_event(event);
                        } else {
                            eprintln!("failed to unpack the upgrade event");
                        }
                    }
                    _ => (),
                }

                Ok(Continue(true))
            },
        )
    }

    fn event_listen_recovery_upgrade(&self) -> Result<(), ClientError> {
        let mut reset = false;

        self.event_listen(
            DaemonStatus::RecoveryUpgrade,
            methods::RECOVERY_UPGRADE_RELEASE_STATUS,
            RECOVERY_RESULT_STR,
            RECOVERY_RESULT_SUCCESS,
            RECOVERY_RESULT_ERROR,
            move |_client, signal| {
                match &*signal.member().unwrap() {
                    signals::RECOVERY_DOWNLOAD_PROGRESS => {
                        let (progress, total) =
                            signal.read2::<u64, u64>().map_err(ClientError::BadResponse)?;

                        print!(
                            "\r{} {}/{} {}",
                            Paint::green("Fetched").bold(),
                            Paint::yellow(progress / 1024).bold(),
                            Paint::yellow(total / 1024).bold(),
                            Paint::green("MiB").bold()
                        );
                        let _ = io::stdout().flush();
                    }
                    signals::RECOVERY_EVENT => {
                        let status = signal.read1::<u8>().map_err(ClientError::BadResponse)?;

                        let message = RecoveryEvent::from_u8(status)
                            .map(<&'static str>::from)
                            .unwrap_or("unknown event");

                        if reset {
                            reset = false;
                            println!();
                        }

                        println!("{}: {}", Paint::green("Recovery event").bold(), message);
                    }
                    signals::RECOVERY_RESULT => {
                        let (status, why) =
                            signal.read2::<u8, &str>().map_err(ClientError::BadResponse)?;

                        if reset {
                            reset = false;
                            println!();
                        }

                        log_result(
                            status,
                            RECOVERY_RESULT_STR,
                            RECOVERY_RESULT_SUCCESS,
                            RECOVERY_RESULT_ERROR,
                            why,
                        );

                        return Ok(Continue(false));
                    }
                    _ => (),
                }

                Ok(Continue(true))
            },
        )
    }

    fn event_listen_release_upgrade(&self) -> Result<bool, ClientError> {
        let recall = &mut false;

        self.event_listen(
            DaemonStatus::ReleaseUpgrade,
            methods::RELEASE_UPGRADE_STATUS,
            UPGRADE_RESULT_STR,
            UPGRADE_RESULT_SUCCESS,
            UPGRADE_RESULT_ERROR,
            |client, signal| {
                match &*signal.member().unwrap() {
                    signals::PACKAGE_FETCH_RESULT => {
                        let (status, why) =
                            signal.read2::<u8, &str>().map_err(ClientError::BadResponse)?;

                        log_result(
                            status,
                            FETCH_RESULT_STR,
                            FETCH_RESULT_SUCCESS,
                            FETCH_RESULT_ERROR,
                            why,
                        );
                    }
                    signals::PACKAGE_FETCHED => {
                        let (name, completed, total) =
                            signal.read3::<&str, u32, u32>().map_err(ClientError::BadResponse)?;

                        println!(
                            "{} ({}/{}): {}",
                            Paint::green("Fetched").bold(),
                            Paint::yellow(completed).bold(),
                            Paint::yellow(total).bold(),
                            Paint::magenta(name).bold()
                        );
                    }
                    signals::PACKAGE_FETCHING => {
                        let name = signal.read1::<&str>().map_err(ClientError::BadResponse)?;

                        println!(
                            "{} {}",
                            Paint::green("Fetching").bold(),
                            Paint::magenta(name).bold()
                        );
                    }
                    signals::PACKAGE_UPGRADE => {
                        let event = signal
                            .read1::<HashMap<&str, String>>()
                            .map_err(ClientError::BadResponse)?;

                        if let Ok(event) = AptUpgradeEvent::from_dbus_map(event) {
                            write_apt_event(event);
                        } else {
                            eprintln!("failed to unpack the upgrade event");
                        }
                    }
                    signals::RELEASE_RESULT => {
                        let (status, why) =
                            signal.read2::<u8, &str>().map_err(ClientError::BadResponse)?;

                        log_result(
                            status,
                            UPGRADE_RESULT_STR,
                            UPGRADE_RESULT_SUCCESS,
                            UPGRADE_RESULT_ERROR,
                            why,
                        );

                        return Ok(Continue(false));
                    }
                    signals::RELEASE_EVENT => {
                        let status = signal.read1::<u8>().map_err(ClientError::BadResponse)?;

                        let message = UpgradeEvent::from_u8(status)
                            .map(<&'static str>::from)
                            .unwrap_or("unknown event");

                        println!("{}: {}", Paint::green("Release Event").bold(), message);
                    }
                    signals::REPO_COMPAT_ERROR => {
                        let (success, failure) = signal
                            .read2::<Vec<&str>, Vec<(&str, &str)>>()
                            .map_err(ClientError::BadResponse)?;

                        println!("{}:", Paint::red("Incompatible repositories detected").bold());

                        for (url, why) in &failure {
                            println!(
                                "    {}: {}: {}",
                                Paint::red("Error").bold(),
                                Paint::cyan(url).bold(),
                                Paint::red(why).bold(),
                            );
                        }

                        for url in success {
                            println!(
                                "    {}: {}",
                                Paint::green("Success").bold(),
                                Paint::cyan(url).bold()
                            );
                        }

                        println!("{}", Paint::yellow("Requesting user input:").bold());

                        let repos = failure.iter()
                            .map(|(url, _)| *url)
                            .map(|url| {
                                let prompt = format!("    Keep repository {}? y/N", url);
                                let res = <Option<bool>>::prompt(prompt).unwrap_or(false);
                                MessageItem::DictEntry(
                                    Box::new(url.into()),
                                    Box::new((res as u8).into())
                                )
                            });

                        let array = MessageItemArray::new(
                            repos.collect::<Vec<_>>(),
                            Signature::from_slice(b"a{sy}\0").unwrap(),
                        )
                        .unwrap();


                        info!("sending message");
                        client.call_method(methods::REPO_MODIFY, iter::once(MessageItem::Array(array)))?;
                        info!("message sent");

                        *recall = true;
                    }
                    _ => (),
                }

                Ok(Continue(true))
            },
        )?;

        Ok(*recall)
    }

    fn status_is(&self, expected: DaemonStatus) -> Result<bool, ClientError> {
        let message = self.call_method("Status", iter::empty())?;
        let status = message.read1::<u8>().map_err(ClientError::BadResponse)?;
        let status = DaemonStatus::from_u8(status).ok_or(ClientError::DaemonStatusOutOfRange)?;
        Ok(status == expected)
    }
}

fn filter_signal(ci: ConnectionItem) -> Option<Message> {
    if let ConnectionItem::Signal(ci) = ci {
        Some(ci)
    } else {
        None
    }
}

fn write_apt_event(event: AptUpgradeEvent) {
    let dpkg = Paint::green("Dpkg").bold();
    match event {
        AptUpgradeEvent::Processing { package } => {
            println!(
                "{}: {} for {}",
                dpkg,
                Paint::cyan("Processing triggers").bold(),
                Paint::magenta(package).bold()
            );
        }
        AptUpgradeEvent::Progress { percent } => {
            println!(
                "{}: {}: {}%",
                dpkg,
                Paint::cyan("Progress").bold(),
                Paint::yellow(percent).bold()
            );
        }
        AptUpgradeEvent::SettingUp { package } => {
            println!(
                "{}: {} {}",
                dpkg,
                Paint::cyan("Setting up").bold(),
                Paint::magenta(package).bold()
            );
        }
        AptUpgradeEvent::Unpacking { package, version, over } => {
            println!(
                "{}: {} {} ({}) over ({})",
                dpkg,
                Paint::cyan("Unpacking").bold(),
                Paint::magenta(package).bold(),
                Paint::yellow(version).bold(),
                Paint::yellow(over).bold()
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
        Paint::cyan(event).bold(),
        if status == 0 {
            Paint::green(success).bold()
        } else {
            inner = format!("{}: {}", Paint::red(error).bold(), Paint::yellow(why).bold());

            Paint::wrapping(inner.as_str())
        }
    );
}
