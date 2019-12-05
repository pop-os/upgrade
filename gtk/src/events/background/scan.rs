use crate::{
    events::{CompletedEvent, InitiatedEvent, ScanEvent, UiEvent},
    users,
};

use pop_upgrade::{
    client::Client,
    release::{self, STARTUP_UPGRADE_FILE},
};

use std::{borrow::Cow, path::Path};

pub fn scan(client: &Client, send: &dyn Fn(UiEvent)) {
    send(UiEvent::Initiated(InitiatedEvent::Scanning));

    let mut upgrade = None;
    let mut is_current = false;
    let mut is_lts = false;
    let mut status_failed = false;

    if !users::is_admin() {
        send(UiEvent::Completed(CompletedEvent::Scan(ScanEvent::PermissionDenied)));
        return;
    }

    let reboot_ready = Path::new(STARTUP_UPGRADE_FILE).exists();

    let upgrade_text = if !reboot_ready && release::upgrade_in_progress() {
        Cow::Borrowed("Pop!_OS is currently downloading.")
    } else {
        let devel = pop_upgrade::development_releases_enabled();
        let result = client.release_check(devel);
        match result {
            Ok(info) => {
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
        is_current,
        is_lts,
        reboot_ready,
        refresh: client.recovery_exists(),
        status_failed,
        upgrade_text: Box::from(upgrade_text.as_ref()),
        upgrade,
    })));
}
