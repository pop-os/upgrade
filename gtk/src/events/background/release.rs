use super::status_changed;
use crate::events::*;

use apt_cmd::AptUpgradeEvent;
use pop_upgrade::{
    client::{self, Client, ReleaseInfo, Signal},
    daemon::DaemonStatus,
    release::UpgradeMethod,
};

pub fn download(client: &Client, send: &dyn Fn(UiEvent), info: &ReleaseInfo) {
    info!("downloading updates for {}", info.next);
    if !update(client, send) {
        return;
    }

    let &ReleaseInfo { ref current, ref next, .. } = &info;
    // TODO: Re-enable this when QA is ready for testing this behavior.
    //    let how = if client.recovery_exists() {
    //        // Upgrade the recovery partition in addition to the OS.
    //        if !upgrade_recovery(client, send, next) {
    //            return;
    //        }
    //
    //        UpgradeMethod::Recovery
    //    } else {
    //        UpgradeMethod::Offline
    //    };

    let how = UpgradeMethod::Offline;

    send(UiEvent::Initiated(InitiatedEvent::Download(next.clone())));

    if let Err(why) = client.release_upgrade(how, current, next) {
        send(UiEvent::Error(UiError::Upgrade(why.into())));
        return;
    }

    let error = &mut None;
    let ignore_error = &mut false;
    let status_broken = &mut false;

    use pop_upgrade::client::Progress;

    let _ = client.event_listen(
        Client::release_upgrade_status,
        |status| {
            *status_broken = true;
            status_changed(send, status, DaemonStatus::ReleaseUpgrade);
        },
        |_client, signal| {
            match signal {
                Signal::PackageFetchResult(status) | Signal::RecoveryResult(status) => {
                    if status.status != 0 {
                        *error = Some(status.why);
                        return Ok(client::Continue(false));
                    }
                }
                Signal::PackageFetched(status) => {
                    send(UiEvent::Progress(ProgressEvent::Fetching(
                        status.completed.into(),
                        status.total.into(),
                    )));
                }
                Signal::PackageUpgrade(event) => {
                    match AptUpgradeEvent::from_dbus_map(event.into_iter()) {
                        Ok(AptUpgradeEvent::Progress { percent }) => {
                            send(UiEvent::Progress(ProgressEvent::Updates(percent)));
                        }
                        Ok(AptUpgradeEvent::WaitingOnLock) => {
                            send(UiEvent::WaitingOnLock);
                        }
                        _ => (),
                    }
                }
                Signal::RecoveryDownloadProgress(Progress { progress, total }) => {
                    println!("Progress {}/{}", progress, total);
                    send(UiEvent::Progress(ProgressEvent::Recovery(progress, total)));
                }
                Signal::RecoveryEvent(event) => {
                    send(UiEvent::Recovery(OsRecoveryEvent::Event(event)));
                }
                Signal::ReleaseEvent(event) => {
                    send(UiEvent::Upgrade(OsUpgradeEvent::Event(event)));
                }
                Signal::ReleaseResult(status) => {
                    if status.status != 0 {
                        *error = Some(status.why);
                    }

                    return Ok(client::Continue(false));
                }
                _ => (),
            }

            Ok(client::Continue(true))
        },
    );

    if *ignore_error {
        return;
    }

    send(if let Some(why) = error.take() {
        UiEvent::Error(UiError::Upgrade(why.into()))
    } else if *status_broken {
        UiEvent::Error(UiError::Upgrade(
            Box::<str>::from("Upgrade service status changed unexpectedly").into(),
        ))
    } else {
        UiEvent::Completed(CompletedEvent::Download)
    });
}

pub fn update(client: &Client, send: &dyn Fn(UiEvent)) -> bool {
    info!("checking if updates are required");
    let updates = match client.fetch_updates(Vec::new(), false) {
        Ok(updates) => updates,
        Err(why) => {
            send(UiEvent::Error(UiError::Updates(why.into())));
            return false;
        }
    };

    if updates.updates_available {
        send(UiEvent::Progress(ProgressEvent::Fetching(
            updates.completed.into(),
            updates.total.into(),
        )));

        let error = &mut None;

        let _ = client.event_listen(
            Client::fetch_updates_status,
            |status| status_changed(send, status, DaemonStatus::FetchingPackages),
            |_client, signal| {
                match signal {
                    Signal::PackageFetchResult(status) => {
                        if status.status != 0 {
                            *error = Some(status.why);
                        }

                        return Ok(client::Continue(false));
                    }
                    Signal::PackageFetched(status) => {
                        send(UiEvent::Progress(ProgressEvent::Fetching(
                            status.completed.into(),
                            status.total.into(),
                        )));
                    }
                    Signal::PackageUpgrade(event) => {
                        match AptUpgradeEvent::from_dbus_map(event.into_iter()) {
                            Ok(AptUpgradeEvent::Progress { percent }) => {
                                send(UiEvent::Progress(ProgressEvent::Updates(percent)));
                            }
                            Ok(AptUpgradeEvent::WaitingOnLock) => {
                                send(UiEvent::WaitingOnLock);
                            }
                            _ => (),
                        }
                    }
                    Signal::ReleaseEvent(event) => {
                        send(UiEvent::Upgrade(OsUpgradeEvent::Event(event)));
                    }
                    _ => (),
                }

                Ok(client::Continue(true))
            },
        );

        if let Some(why) = error.take() {
            send(UiEvent::Error(UiError::Updates(why.into())));
            return false;
        }
    }

    true
}
