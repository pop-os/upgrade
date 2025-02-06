use crate::{
    events::{CompletedEvent, InitiatedEvent, UiEvent},
    fl, users,
};

use pop_upgrade::{
    client::{Client, Error as ClientError, ReleaseInfo},
    release::{self, STARTUP_UPGRADE_FILE},
};
use pop_upgrade_client::DaemonStatus;

use std::path::Path;

#[derive(Debug)]
pub enum ScanEvent {
    Found {
        is_current: bool,
        is_lts: bool,
        refresh: bool,
        status_failed: bool,
        reboot_ready: bool,
        upgrading_recovery: bool,
        urgent: bool,

        current: Option<Box<str>>,
        upgrade_text: Box<str>,

        upgrade: Option<ReleaseInfo>,
    },
    PermissionDenied,
}

fn daemon_status_is(client: &Client, expected: DaemonStatus) -> Result<bool, ClientError> {
    client.status().map(|actual| expected as u8 == actual.status)
}

pub fn scan(client: &Client, send: &dyn Fn(UiEvent)) {
    send(UiEvent::Initiated(InitiatedEvent::Scanning));

    let mut current = None;
    let mut upgrade = None;
    let mut is_current = false;
    let mut is_lts = false;
    let mut status_failed = false;
    let mut urgent = false;

    if !users::is_admin() {
        send(UiEvent::Completed(CompletedEvent::Scan(ScanEvent::PermissionDenied)));
        return;
    }

    let reboot_ready = Path::new(STARTUP_UPGRADE_FILE).exists();

    let upgrading_recovery =
        daemon_status_is(client, DaemonStatus::RecoveryUpgrade).unwrap_or(false);

    let upgrade_text: String = if !reboot_ready && pop_upgrade_client::upgrade_in_progress() {
        fl!("upgrade-downloading")
    } else {
        let devel = pop_upgrade_client::development_releases_enabled();
        let result = client.release_check(devel);
        match result {
            Ok(info) => {
                current = dbg!(Some(info.current.clone()));
                match client.recovery_version() {
                    Ok(rinfo) => {
                        urgent = info.urgent.map_or(false, |urgent| {
                            rinfo.version != info.current
                                || rinfo.build < 0
                                || (rinfo.build as u16) < urgent
                        }) || rinfo.version != info.current;
                    }
                    Err(_) => {
                        urgent = info.urgent.unwrap_or(0) > 0;
                    }
                }

                is_lts = info.is_lts;
                if devel || info.build >= 0 {
                    info!(
                        "{}",
                        fl!("upgrade-from-to", current = (&*info.current), next = (&*info.next))
                    );

                    let upgrade_text = if reboot_ready {
                        fl!("upgrade-ready", version = (&*info.next))
                    } else {
                        fl!("upgrade-available", version = (&*info.next))
                    };

                    upgrade = Some(info);
                    upgrade_text
                } else {
                    status_failed = true;
                    match info.build {
                        -1 => fl!("error-build-status"),
                        -2 | -4 => {
                            is_current = true;
                            status_failed = false;
                            fl!("release-current")
                        }
                        -3 => fl!("error-connection"),
                        _ => fl!("error-unknown-status"),
                    }
                }
            }
            Err(why) => {
                status_failed = true;
                let msg = fl!("error-update-check");
                error!("{}: {}", msg, why);
                msg
            }
        }
    };

    send(UiEvent::Completed(CompletedEvent::Scan(ScanEvent::Found {
        current,
        is_current,
        is_lts,
        reboot_ready,
        refresh: client.recovery_exists(),
        status_failed,
        upgrade_text: Box::from(upgrade_text),
        upgrade,
        upgrading_recovery,
        urgent,
    })));

    if upgrading_recovery {
        super::recovery::upgrade_listen(client, send);
    }
}
