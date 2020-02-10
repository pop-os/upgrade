use crate::{
    errors::UiError,
    events::{CompletedEvent, InitiatedEvent, OsRecoveryEvent, ProgressEvent, UiEvent},
};

use super::status_changed;

use pop_upgrade::{
    client::{self, Client, Signal},
    daemon::DaemonStatus,
    recovery::ReleaseFlags,
};

pub fn upgrade(client: &mut Client, send: &dyn Fn(UiEvent), version: &str) -> bool {
    send(UiEvent::Initiated(InitiatedEvent::Recovery));

    let arch = "nvidia";
    let flags = ReleaseFlags::empty();

    if let Err(why) = client.recovery_upgrade_release(version, arch, flags) {
        send(UiEvent::Error(UiError::Recovery(why.into())));
        return false;
    }

    let error = &mut None;

    let _ = client.event_listen(
        DaemonStatus::RecoveryUpgrade,
        Client::recovery_upgrade_release_status,
        |status| status_changed(send, status, DaemonStatus::RecoveryUpgrade),
        |_client, signal| {
            match signal {
                Signal::RecoveryDownloadProgress(progress) => {
                    send(UiEvent::Progress(ProgressEvent::Recovery(
                        progress.progress,
                        progress.total,
                    )));
                }
                Signal::RecoveryEvent(event) => {
                    send(UiEvent::Recovery(OsRecoveryEvent::Event(event)));
                }
                Signal::RecoveryResult(status) => {
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

    if let Some(why) = error.take() {
        send(UiEvent::Error(UiError::Recovery(why.into())));
        return false;
    }

    send(UiEvent::Completed(CompletedEvent::Recovery));
    true
}
