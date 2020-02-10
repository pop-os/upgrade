pub mod recovery;
pub mod release;
pub mod scan;

use crate::{errors::UnderlyingError, events::*, reboot};

use self::scan::scan;

use num_traits::cast::FromPrimitive;
use pop_upgrade::{
    client::{Client, ReleaseInfo, Status},
    daemon::{DaemonStatus, DismissEvent},
    release::RefreshOp,
};

use std::sync::mpsc::{self, SyncSender};

/// Events sent to this widget's background thread.
#[derive(Debug)]
pub enum BackgroundEvent {
    DownloadUpgrade(ReleaseInfo),
    Finalize,
    GetStatus(DaemonStatus),
    IsActive(SyncSender<bool>),
    DismissNotification(bool),
    RefreshOS,
    RepoModify(Vec<Box<str>>, Vec<bool>),
    Reset,
    Scan,
    Shutdown,
    UpdateRecovery(Box<str>),
}

pub fn run(
    receiver: mpsc::Receiver<BackgroundEvent>,
    send: impl Fn(UiEvent) + Send + Sync + 'static,
) {
    let send: &dyn Fn(UiEvent) = &send;
    if let Ok(ref mut client) = Client::new() {
        while let Ok(event) = receiver.recv() {
            trace!("received background event: {:?}", event);
            match event {
                BackgroundEvent::DismissNotification(dismiss) => {
                    let dismiss_event =
                        if dismiss { DismissEvent::ByUser } else { DismissEvent::Unset };

                    let event = match client.dismiss_notification(dismiss_event) {
                        Ok(dismissed) => UiEvent::Upgrade(OsUpgradeEvent::Dismissed(dismissed)),
                        Err(why) => {
                            UiEvent::Error(UiError::Dismiss(dismiss, UnderlyingError::Client(why)))
                        }
                    };

                    send(event)
                }

                BackgroundEvent::DownloadUpgrade(info) => {
                    self::release::download(client, send, info);
                }

                BackgroundEvent::Finalize => match client.release_upgrade_finalize() {
                    Ok(()) => reboot(),
                    Err(why) => send(UiEvent::Error(UiError::Finalize(why))),
                },

                BackgroundEvent::GetStatus(from) => {
                    get_status(client, send, from);
                }

                BackgroundEvent::IsActive(tx) => {
                    let _ = tx.send(client.status().is_ok());
                }

                BackgroundEvent::RefreshOS => {
                    refresh_os(client, send);
                }

                BackgroundEvent::RepoModify(failures, answers) => {
                    repo_modify(client, send, failures, answers);
                }

                BackgroundEvent::Reset => {
                    send(match client.reset() {
                        Ok(()) => UiEvent::Upgrade(OsUpgradeEvent::Cancelled),
                        Err(why) => UiEvent::Error(UiError::Cancel(why)),
                    });
                }

                BackgroundEvent::Scan => scan(client, send),

                BackgroundEvent::Shutdown => {
                    send(UiEvent::Shutdown);
                    break;
                }

                BackgroundEvent::UpdateRecovery(version) => {
                    self::recovery::upgrade(client, send, &version);
                }
            }
        }
    }
}

fn status_recovery_upgrade(client: &Client) -> UiEvent {
    match client.recovery_upgrade_release_status() {
        Ok(status) => {
            if status.status == 0 {
                UiEvent::Completed(CompletedEvent::Recovery)
            } else {
                UiEvent::Error(UiError::Recovery(status.why.into()))
            }
        }
        Err(why) => UiEvent::Error(UiError::Recovery(why.into())),
    }
}

fn status_release_upgrade(client: &Client) -> UiEvent {
    match client.release_upgrade_status() {
        Ok(status) => {
            if status.status == 0 {
                UiEvent::Completed(CompletedEvent::Download)
            } else {
                UiEvent::Error(UiError::Upgrade(status.why.into()))
            }
        }
        Err(why) => UiEvent::Error(UiError::Upgrade(why.into())),
    }
}

fn get_status(client: &Client, send: &dyn Fn(UiEvent), from: DaemonStatus) {
    match from {
        DaemonStatus::RecoveryUpgrade => send(status_recovery_upgrade(client)),
        DaemonStatus::ReleaseUpgrade => send(status_release_upgrade(client)),
        _ => (),
    }
}

fn refresh_os(client: &Client, send: &dyn Fn(UiEvent)) {
    send(UiEvent::Initiated(InitiatedEvent::Refresh));

    if let Err(why) = client.refresh_os(RefreshOp::Enable) {
        send(UiEvent::Error(UiError::Refresh(why.into())));
        return;
    }

    send(UiEvent::Completed(CompletedEvent::Refresh));
}

fn repo_modify(
    client: &Client,
    send: &dyn Fn(UiEvent),
    failures: Vec<Box<str>>,
    answers: Vec<bool>,
) {
    let input = failures.into_iter().zip(answers.into_iter());
    if let Err(why) = client.repo_modify(input) {
        send(UiEvent::Error(UiError::Repos(why.into())));
        return;
    }

    send(UiEvent::Upgrade(OsUpgradeEvent::Upgrade));
}

fn status_changed(send: &dyn Fn(UiEvent), new_status: Status, expected: DaemonStatus) {
    let status = DaemonStatus::from_u8(new_status.status).expect("unknown daemon status value");
    send(UiEvent::StatusChanged(expected, status, new_status.why));
}
