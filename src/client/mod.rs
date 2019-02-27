use crate::daemon::*;
use crate::recovery::{RecoveryEvent, ReleaseFlags as RecoveryReleaseFlags};
use crate::release::{UpgradeEvent, UpgradeMethod};
use crate::{DBUS_IFACE, DBUS_NAME, DBUS_PATH};
use clap::ArgMatches;
use dbus::{self, BusType, Connection, ConnectionItem, Message, MessageItem};
use num_traits::FromPrimitive;
use std::io::{self, Write};
use std::iter;

const TIMEOUT: i32 = 3000;

pub struct Continue(pub bool);

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
            add_match(&bus, signals::PACKAGE_FETCH_RESULT)?;
            add_match(&bus, signals::PACKAGE_FETCHED)?;
            add_match(&bus, signals::PACKAGE_FETCHING)?;
            add_match(&bus, signals::RECOVERY_DOWNLOAD_PROGRESS)?;
            add_match(&bus, signals::RECOVERY_RESULT)?;
            add_match(&bus, signals::RECOVERY_EVENT)?;
            add_match(&bus, signals::RELEASE_RESULT)?;
            add_match(&bus, signals::RELEASE_EVENT)?;

            Ok(Client { bus })
        })
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

                        let flags: u8 = flags.bits();
                        let args: Vec<MessageItem> =
                            vec![version.into(), arch.into(), flags.into()];

                        self.call_method(methods::RECOVERY_UPGRADE_RELEASE, args.into_iter())
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
                let (current, next, available) = self.release_check(&mut message)?;

                println!(
                    "      Current Release: {}\n         Next Release: {}\nNew Release Available: {}",
                    current, next, available
                );
            }
            ("update", Some(matches)) => {
                let args = iter::once(matches.is_present("download-only").into());
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
            ("upgrade", Some(matches)) => {
                let method = match matches.subcommand() {
                    ("offline", _) => UpgradeMethod::Offline,
                    ("recovery", _) => UpgradeMethod::Recovery,
                    _ => unreachable!(),
                };

                let mut message = None;
                let (current, next, available) = self.release_check(&mut message)?;

                if available {
                    let args = vec![(method as u8).into(), current.into(), next.into()];
                    let _message = self.call_method(methods::RELEASE_UPGRADE, args.into_iter())?;
                    self.event_listen_release_upgrade()?;
                } else {
                    println!("no release available to upgrade to");
                }
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

    #[cfg(features = "testing")]
    pub fn testing(&self, matches: &ArgMatches) -> Result<(), ClientError> {
        match matches.subcommand() {
            ("upgrade", _) => {
                self.call_method(methods::PACKAGE_UPGRADE, iter::empty())?;
                self.event_listen_test();
            }
            _ => unreachable!()
        }
    }

    #[cfg(not(features = "testing"))]
    pub fn testing(&self, _matches: &ArgMatches) -> Result<(), ClientError> {
        Ok(())
    }

    fn release_check<'a>(
        &self,
        message: &'a mut Option<Message>,
    ) -> Result<(&'a str, &'a str, bool), ClientError> {
        *message = Some(self.call_method(methods::RELEASE_CHECK, iter::empty())?);
        message.as_mut().unwrap().read3::<&str, &str, bool>().map_err(ClientError::BadResponse)
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
        mut event: F,
    ) -> Result<(), ClientError>
    where
        F: FnMut(&Self, Message) -> Result<Continue, ClientError>,
    {
        for item in self.bus.iter(3000) {
            if let ConnectionItem::Nothing = item {
                if !self.status_is(expected_status)? {
                    warn!("daemon status changed before getting the result");
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
        self.event_listen(DaemonStatus::FetchingPackages, |_client, signal| {
            match &*signal.member().unwrap() {
                signals::PACKAGE_FETCH_RESULT => {
                    let status = signal.read1::<u8>().map_err(ClientError::BadResponse)?;

                    println!("package fetching complete: status was {}", status);
                    return Ok(Continue(false));
                }
                signals::PACKAGE_FETCHED => {
                    let (name, completed, total) =
                        signal.read3::<&str, u32, u32>().map_err(ClientError::BadResponse)?;

                    println!("{}/{}: fetched {}", completed, total, name);
                }
                signals::PACKAGE_FETCHING => {
                    let name = signal.read1::<&str>().map_err(ClientError::BadResponse)?;

                    println!("fetching {}", name);
                }
                _ => (),
            }

            Ok(Continue(true))
        })
    }

    fn event_listen_recovery_upgrade(&self) -> Result<(), ClientError> {
        let mut reset = false;

        self.event_listen(DaemonStatus::RecoveryUpgrade, move |_client, signal| {
            match &*signal.member().unwrap() {
                signals::RECOVERY_DOWNLOAD_PROGRESS => {
                    let (progress, total) =
                        signal.read2::<u64, u64>().map_err(ClientError::BadResponse)?;

                    print!("\rISO downloaded {} of {} MiB", progress / 1024, total / 1024);
                    let _ = io::stdout().flush();
                }
                signals::RECOVERY_EVENT => {
                    let status = signal.read1::<u8>().map_err(ClientError::BadResponse)?;

                    let message = RecoveryEvent::from_u8(status)
                        .map(<&'static str>::from)
                        .unwrap_or("unknown event");

                    if reset {
                        reset = false;
                        println!("");
                    }

                    println!("recovery event: {}", message);
                }
                signals::RECOVERY_RESULT => {
                    let status = signal.read1::<u8>().map_err(ClientError::BadResponse)?;

                    if reset {
                        reset = false;
                        println!("");
                    }

                    println!("recovery upgrade complete: status was {}", status);
                    return Ok(Continue(false));
                }
                _ => (),
            }

            Ok(Continue(true))
        })
    }

    fn event_listen_release_upgrade(&self) -> Result<(), ClientError> {
        self.event_listen(DaemonStatus::ReleaseUpgrade, |_client, signal| {
            match &*signal.member().unwrap() {
                signals::PACKAGE_FETCH_RESULT => {
                    let status = signal.read1::<u8>().map_err(ClientError::BadResponse)?;

                    println!("package fetching complete: status was {}", status);
                    return Ok(Continue(false));
                }
                signals::PACKAGE_FETCHED => {
                    let (name, completed, total) =
                        signal.read3::<&str, u32, u32>().map_err(ClientError::BadResponse)?;

                    println!("{}/{}: fetched {}", completed, total, name);
                }
                signals::PACKAGE_FETCHING => {
                    let name = signal.read1::<&str>().map_err(ClientError::BadResponse)?;

                    println!("fetching {}", name);
                }
                signals::RELEASE_RESULT => {
                    let status = signal.read1::<u8>().map_err(ClientError::BadResponse)?;

                    println!("recovery upgrade complete: status was {}", status);
                    return Ok(Continue(false));
                }
                signals::RELEASE_EVENT => {
                    let status = signal.read1::<u8>().map_err(ClientError::BadResponse)?;

                    let message = UpgradeEvent::from_u8(status)
                        .map(<&'static str>::from)
                        .unwrap_or("unknown event");

                    println!("release upgrade event: {}", message);
                }
                _ => (),
            }

            Ok(Continue(true))
        })
    }

    #[cfg(features = "testing")]
    fn event_listen_release_test(&self) -> Result<(), ClientError> {
        self.event_listen(DaemonStatus::PackageUpgrade, |_client, signal| {
            match &*signal.member().unwrap() {
                signals::PACKAGE_UPGRADE => {
                    let event = signal.read1::<HashMap<&str, String>>().map_err(ClientError::BadResponse)?;
                    println!("PACKAGE_UPGRADE: {:?}", event);
                    return Ok(Continue(false));
                }
                _ => (),
            }

            Ok(Continue(true))
        })
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
