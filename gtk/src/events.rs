use crate::errors::UiError;
use pop_upgrade::{
    client::{ReleaseInfo, RepoCompatError},
    daemon::DaemonStatus,
    release::UpgradeEvent,
};
use std::sync::mpsc::SyncSender;

/// Events sent to this widget's background thread.
#[derive(Debug)]
pub enum BackgroundEvent {
    DownloadUpgrade(ReleaseInfo),
    GetStatus(DaemonStatus),
    IsActive(SyncSender<bool>),
    RefreshOS,
    RepoModify(Vec<Box<str>>, Vec<bool>),
    Scan,
    Shutdown,
}

/// Events received for the UI to handle.
#[derive(Debug)]
pub enum UiEvent {
    Completed(CompletedEvent),
    Error(UiError),
    IncompatibleRepos(RepoCompatError),
    Initiated(InitiatedEvent),
    Progress(ProgressEvent),
    Shutdown,
    StatusChanged(DaemonStatus, DaemonStatus, Box<str>),
    UpgradeClicked,
    UpgradeEvent(UpgradeEvent),
    WaitingOnLock,
}

#[derive(Debug)]
pub enum InitiatedEvent {
    Download(Box<str>),
    Recovery,
    Refresh,
    Scanning,
}

#[derive(Debug)]
pub enum CompletedEvent {
    Download,
    Recovery,
    Refresh,
    Scan {
        is_lts:        bool,
        refresh:       bool,
        status_failed: bool,
        upgrade_text:  Box<str>,
        upgrade:       Option<ReleaseInfo>,
    },
}

#[derive(Debug)]
pub enum ProgressEvent {
    Fetching(u64, u64),
    Recovery(u64, u64),
    Updates(u8),
}
