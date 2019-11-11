use super::status_changed;
use crate::events::*;

use apt_cli_wrappers::AptUpgradeEvent;
use pop_upgrade::{
    client::{self, Client, ReleaseInfo, Signal},
    daemon::DaemonStatus,
    release::UpgradeMethod,
};

pub fn download(client: &Client, send: &dyn Fn(UiEvent), info: ReleaseInfo) {
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

    let _ = client.event_listen(
        DaemonStatus::ReleaseUpgrade,
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
                        status.completed as u64,
                        status.total as u64,
                    )));
                }
                Signal::PackageUpgrade(event) => {
                    match AptUpgradeEvent::from_dbus_map(event.into_iter()) {
                        Ok(AptUpgradeEvent::Progress { percent }) => {
                            send(UiEvent::Progress(ProgressEvent::Updates(percent)))
                        }
                        Ok(AptUpgradeEvent::WaitingOnLock) => {
                            send(UiEvent::WaitingOnLock);
                        }
                        _ => (),
                    }
                }
                Signal::ReleaseEvent(event) => {
                    send(UiEvent::UpgradeEvent(event));
                }
                Signal::ReleaseResult(status) => {
                    if status.status != 0 {
                        *error = Some(status.why);
                    }

                    return Ok(client::Continue(false));
                }
                Signal::RecoveryDownloadProgress(progress) => {
                    send(UiEvent::Progress(ProgressEvent::Recovery(
                        progress.progress,
                        progress.total,
                    )));
                }
                Signal::RepoCompatError(repositories) => {
                    *ignore_error = true;
                    send(UiEvent::IncompatibleRepos(repositories));
                }
                _ => (),
            }

            Ok(client::Continue(true))
        },
    );

    if *ignore_error {
        return;
    }

    if let Some(why) = error.take() {
        send(UiEvent::Error(UiError::Upgrade(why.into())));
        return;
    }

    if !*status_broken {
        send(UiEvent::Completed(CompletedEvent::Download));
    }
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
            updates.completed as u64,
            updates.total as u64,
        )));

        let error = &mut None;

        debug!("listening for package fetching signals");
        let _ = client.event_listen(
            DaemonStatus::FetchingPackages,
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
                            status.completed as u64,
                            status.total as u64,
                        )));
                    }
                    Signal::PackageUpgrade(event) => {
                        match AptUpgradeEvent::from_dbus_map(event.into_iter()) {
                            Ok(AptUpgradeEvent::Progress { percent }) => {
                                send(UiEvent::Progress(ProgressEvent::Updates(percent)))
                            }
                            Ok(AptUpgradeEvent::WaitingOnLock) => {
                                send(UiEvent::WaitingOnLock);
                            }
                            _ => (),
                        }
                    }
                    Signal::ReleaseEvent(event) => {
                        send(UiEvent::UpgradeEvent(event));
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
