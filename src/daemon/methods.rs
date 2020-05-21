use crate::{
    daemon::{dbus_helper::DbusFactory, Daemon, DaemonStatus},
    release::RefreshOp,
};

use dbus::{
    self,
    tree::{MTFn, Method},
};
use num_traits::FromPrimitive;
use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::atomic::Ordering};

use super::result_signal;

// Methods supported by the daemon.
pub const CANCEL: &str = "Cancel";

pub fn cancel(daemon: Rc<RefCell<Daemon>>, dbus_factory: &DbusFactory) -> Method<MTFn<()>, ()> {
    let method = dbus_factory.method::<_, String>(CANCEL, move |_| {
        daemon.borrow_mut().cancel();
        Ok(Vec::new())
    });

    method.consume()
}

pub const DISMISS_NOTIFICATION: &str = "DismissNotification";

#[repr(u8)]
#[derive(Clone, Copy, Debug, FromPrimitive, PartialEq)]
pub enum DismissEvent {
    ByTimestamp = 1,
    ByUser = 2,
    Unset = 3,
}

pub fn dismiss_notification(
    daemon: Rc<RefCell<Daemon>>,
    dbus_factory: &DbusFactory,
) -> Method<MTFn<()>, ()> {
    dbus_factory
        .method::<_, String>(DISMISS_NOTIFICATION, move |message| {
            let value = message.read1().map_err(|why| format!("{}", why))?;

            let event = DismissEvent::from_u8(value).ok_or("dismiss value is out of range")?;

            let dismissed = daemon.borrow().dismiss_notification(event)?;
            Ok(vec![dismissed.into()])
        })
        .inarg::<u8>("dismiss")
        .inarg::<&str>("timestamp")
        .outarg::<bool>("dismissed")
        .consume()
}

pub const FETCH_UPDATES: &str = "FetchUpdates";

pub fn fetch_updates(
    daemon: Rc<RefCell<Daemon>>,
    dbus_factory: &DbusFactory,
) -> Method<MTFn<()>, ()> {
    let method = dbus_factory.method(FETCH_UPDATES, move |message| {
        let mut daemon = daemon.borrow_mut();
        daemon.set_status(DaemonStatus::FetchingPackages, move |daemon, already_active| {
            if already_active {
                let (completed, total) = daemon.fetching_state.load(Ordering::SeqCst);
                let completed = completed as u32;
                let total = total as u32;
                Ok(vec![true.into(), completed.into(), total.into()])
            } else {
                let (value, download_only): (Vec<String>, bool) =
                    message.read2().map_err(|why| format!("{}", why))?;

                daemon
                    .fetch_updates(&value, download_only)
                    .map(|(x, t)| vec![x.into(), 0u32.into(), t.into()])
            }
        })
    });

    method
        .inarg::<Vec<String>>("additional_packages")
        .inarg::<bool>("download_only")
        .outarg::<bool>("updates_available")
        .outarg::<u32>("completed")
        .outarg::<u32>("total")
        .consume()
}

pub const FETCH_UPDATES_STATUS: &str = "FetchUpdatesStatus";

pub fn fetch_updates_status(
    daemon: Rc<RefCell<Daemon>>,
    dbus_factory: &DbusFactory,
) -> Method<MTFn<()>, ()> {
    let method = dbus_factory.method::<_, u8>(FETCH_UPDATES_STATUS, move |_| {
        let (status, why) = result_signal(daemon.borrow().last_known.fetch.as_ref());
        Ok(vec![status.into(), why.into()])
    });

    method.outarg::<u8>("status").outarg::<&str>("why").consume()
}

pub const PACKAGE_UPGRADE: &str = "UpgradePackages";

pub fn package_upgrade(
    daemon: Rc<RefCell<Daemon>>,
    dbus_factory: &DbusFactory,
) -> Method<MTFn<()>, ()> {
    let method = dbus_factory.method::<_, String>(PACKAGE_UPGRADE, move |_| {
        daemon.borrow_mut().set_status(DaemonStatus::PackageUpgrade, move |daemon, active| {
            if !active {
                daemon.package_upgrade()?;
            }

            Ok(Vec::new())
        })
    });

    method.consume()
}

pub const RECOVERY_UPGRADE_FILE: &str = "RecoveryUpgradeFile";

pub fn recovery_upgrade_file(
    daemon: Rc<RefCell<Daemon>>,
    dbus_factory: &DbusFactory,
) -> Method<MTFn<()>, ()> {
    let method = dbus_factory.method::<_, String>(RECOVERY_UPGRADE_FILE, move |message| {
        let mut daemon = daemon.borrow_mut();
        daemon.set_status(DaemonStatus::RecoveryUpgrade, move |daemon, active| {
            if !active {
                let path = message.read1().map_err(|why| format!("{}", why))?;
                daemon.recovery_upgrade_file(path)?;
            }

            Ok(Vec::new())
        })
    });

    method.inarg::<&str>("path").outarg::<u8>("result").consume()
}

pub const RECOVERY_UPGRADE_RELEASE: &str = "RecoveryUpgradeRelease";

pub fn recovery_upgrade_release(
    daemon: Rc<RefCell<Daemon>>,
    dbus_factory: &DbusFactory,
) -> Method<MTFn<()>, ()> {
    let method = dbus_factory.method::<_, String>(RECOVERY_UPGRADE_RELEASE, move |message| {
        let mut daemon = daemon.borrow_mut();
        daemon.set_status(DaemonStatus::RecoveryUpgrade, move |daemon, active| {
            if !active {
                let (version, arch, flags) = message.read3().map_err(|why| format!("{}", why))?;
                daemon.recovery_upgrade_release(version, arch, flags)?;
            }

            Ok(Vec::new())
        })
    });

    method
        .inarg::<&str>("version")
        .inarg::<&str>("arch")
        .inarg::<u8>("flags")
        .outarg::<u8>("result")
        .consume()
}

pub const RECOVERY_UPGRADE_RELEASE_STATUS: &str = "RecoveryUpgradeReleaseStatus";

pub fn recovery_upgrade_status(
    daemon: Rc<RefCell<Daemon>>,
    dbus_factory: &DbusFactory,
) -> Method<MTFn<()>, ()> {
    let method = dbus_factory.method::<_, u8>(RECOVERY_UPGRADE_RELEASE_STATUS, move |_| {
        let (status, why) = result_signal(daemon.borrow().last_known.recovery_upgrade.as_ref());
        Ok(vec![status.into(), why.into()])
    });

    method.outarg::<u8>("status").outarg::<&str>("why").consume()
}

pub const RECOVERY_VERSION: &str = "RecoveryVersion";

pub fn recovery_version(
    daemon: Rc<RefCell<Daemon>>,
    dbus_factory: &DbusFactory,
) -> Method<MTFn<()>, ()> {
    let method = dbus_factory.method(RECOVERY_VERSION, move |_message| {
        daemon
            .borrow_mut()
            .recovery_version()
            .map(|version| vec![version.version.into(), version.build.into()])
    });

    method.outarg::<&str>("version").outarg::<u16>("build").consume()
}

pub const REFRESH_OS: &str = "RefreshOS";

pub fn refresh_os(daemon: Rc<RefCell<Daemon>>, dbus_factory: &DbusFactory) -> Method<MTFn<()>, ()> {
    let method = dbus_factory.method::<_, String>(REFRESH_OS, move |message| {
        let enable = message.read1().map_err(|why| format!("{}", why))?;
        let value = daemon.borrow_mut().refresh_os(match enable {
            1u8 => RefreshOp::Enable,
            2u8 => RefreshOp::Disable,
            _ => RefreshOp::Status,
        })?;

        info!("responding with value of {}", value);

        Ok(vec![value.into()])
    });

    method.inarg::<u8>("input").outarg::<bool>("enabled").consume()
}

pub const RELEASE_CHECK: &str = "ReleaseCheck";

pub fn release_check(
    daemon: Rc<RefCell<Daemon>>,
    dbus_factory: &DbusFactory,
) -> Method<MTFn<()>, ()> {
    let method = dbus_factory.method(RELEASE_CHECK, move |message| {
        let development = message.read1().map_err(|why| format!("{}", why))?;
        daemon.borrow_mut().release_check(development).map(|status| {
            let is_lts = status.is_lts();
            vec![
                String::from(status.current).into(),
                String::from(status.next).into(),
                status.build.status_code().into(),
                is_lts.into(),
            ]
        })
    });

    method
        .outarg::<&str>("current")
        .outarg::<&str>("next")
        .outarg::<i16>("build")
        .outarg::<bool>("is_lts")
        .consume()
}

pub const RELEASE_UPGRADE: &str = "ReleaseUpgrade";

pub fn release_upgrade(
    daemon: Rc<RefCell<Daemon>>,
    dbus_factory: &DbusFactory,
) -> Method<MTFn<()>, ()> {
    let method = dbus_factory.method::<_, String>(RELEASE_UPGRADE, move |message| {
        let mut daemon = daemon.borrow_mut();
        daemon.set_status(DaemonStatus::ReleaseUpgrade, move |daemon, active| {
            if !active {
                let (how, from, to) = message.read3().map_err(|why| format!("{}", why))?;
                daemon.release_upgrade(how, from, to)?;
            }

            Ok(Vec::new())
        })
    });

    method.inarg::<u8>("how").inarg::<&str>("from").inarg::<&str>("to").consume()
}

pub const RELEASE_UPGRADE_FINALIZE: &str = "ReleaseUpgradeFinalize";

pub fn release_upgrade_finalize(
    daemon: Rc<RefCell<Daemon>>,
    dbus_factory: &DbusFactory,
) -> Method<MTFn<()>, ()> {
    let method = dbus_factory.method::<_, String>(RELEASE_UPGRADE_FINALIZE, move |_| {
        daemon.borrow_mut().release_upgrade_finalize()?;
        Ok(Vec::new())
    });

    method.inarg::<u8>("how").inarg::<&str>("from").inarg::<&str>("to").consume()
}

pub const RELEASE_UPGRADE_STATUS: &str = "ReleaseUpgradeStatus";

pub fn release_upgrade_status(
    daemon: Rc<RefCell<Daemon>>,
    dbus_factory: &DbusFactory,
) -> Method<MTFn<()>, ()> {
    let method = dbus_factory.method::<_, u8>(RELEASE_UPGRADE_STATUS, move |_| {
        let (status, why) = result_signal(daemon.borrow().last_known.release_upgrade.as_ref());
        Ok(vec![status.into(), why.into()])
    });

    method.outarg::<u8>("status").outarg::<&str>("why").consume()
}

pub const RELEASE_REPAIR: &str = "ReleaseRepair";

pub fn release_repair(
    daemon: Rc<RefCell<Daemon>>,
    dbus_factory: &DbusFactory,
) -> Method<MTFn<()>, ()> {
    let method = dbus_factory.method::<_, String>(RELEASE_REPAIR, move |_message| {
        let mut daemon = daemon.borrow_mut();
        daemon.release_repair()?;
        Ok(Vec::new())
    });

    method.consume()
}

pub const RESET: &str = "Reset";

pub fn reset(daemon: Rc<RefCell<Daemon>>, dbus_factory: &DbusFactory) -> Method<MTFn<()>, ()> {
    let method = dbus_factory.method::<_, String>(RESET, move |_| {
        daemon.borrow_mut().reset()?;
        Ok(Vec::new())
    });

    method.inarg::<HashMap<&str, &str>>("repos").consume()
}

pub const STATUS: &str = "Status";

pub fn status(daemon: Rc<RefCell<Daemon>>, dbus_factory: &DbusFactory) -> Method<MTFn<()>, ()> {
    let method = dbus_factory.method::<_, String>(STATUS, move |_| {
        let daemon = daemon.borrow_mut();
        let status = daemon.status.load(Ordering::SeqCst) as u8;
        let sub_status = daemon.sub_status.load(Ordering::SeqCst) as u8;

        Ok(vec![status.into(), sub_status.into()])
    });

    method.outarg::<u8>("status").outarg::<u8>("sub_status").consume()
}
