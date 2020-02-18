use crate::{
    events::{CompletedEvent, InitiatedEvent, UiEvent},
    users,
};

use pop_upgrade::{
    client::{Client, Error as ClientError, ReleaseInfo},
    daemon::DaemonStatus,
    release::{self, STARTUP_UPGRADE_FILE},
};

use std::{borrow::Cow, path::Path};

#[derive(Debug)]
pub enum ScanEvent {
    Found {
        is_current:         bool,
        is_lts:             bool,
        refresh:            bool,
        status_failed:      bool,
        reboot_ready:       bool,
        upgrading_recovery: bool,
        urgent:             bool,

        current:      Option<Box<str>>,
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

    let upgrade_text = if !reboot_ready && release::upgrade_in_progress() {
        Cow::Borrowed("Pop!_OS is currently downloading.")
    } else {
        let devel = pop_upgrade::development_releases_enabled();
        let result = client.release_check(devel);
        match result {
            Ok(info) => {
                current = dbg!(Some(info.current.clone()));
                urgent = info.urgent != -1;

                is_lts = info.is_lts;
                if devel || info.build >= 0 {
                    info!("upgrade from {} to {} is available", info.current, info.next);

                    let upgrade_text = Cow::Owned(if reboot_ready {
                        format!("Pop!_OS is ready to upgrade to {}", info.next)
                    } else {
                        format!("Pop!_OS {} is available.", info.next)
                    });
                    upgrade = Some(info);
                    upgrade_text
                } else {
                    status_failed = true;
                    Cow::Borrowed(match info.build {
                        -1 => "Failed to retrieve build status due to an internal error.",
                        -2 | -4 => {
                            is_current = true;
                            status_failed = false;
                            "You are running the most current Pop!_OS version."
                        }
                        -3 => "Connection failed. You may be offline.",
                        _ => "Unknown status received.",
                    })
                }
            }
            Err(why) => {
                status_failed = true;
                error!("failed to check for updates: {}", why);
                Cow::Borrowed("Failed to check for updates")
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
        upgrade_text: Box::from(upgrade_text.as_ref()),
        upgrade,
        upgrading_recovery,
        urgent,
    })));
}
