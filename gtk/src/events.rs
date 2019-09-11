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
    GetStatus(DaemonStatus),
    IsActive(SyncSender<bool>),
    DismissNotification,
    RefreshOS,
    RepoModify(Vec<Box<str>>, Vec<bool>),
    Scan,
    DownloadUpgrade(ReleaseInfo),
    Shutdown,
}

/// Events received for the UI to handle.
#[derive(Debug)]
pub enum UiEvent {
    Progress(ProgressEvent),
    UpgradeEvent(UpgradeEvent),
    Initiated(InitiatedEvent),
    Completed(CompletedEvent),
    Dismissed,
    Shutdown,
    UpgradeClicked,
    IncompatibleRepos(RepoCompatError),
    StatusChanged(DaemonStatus, DaemonStatus, Box<str>),
    Error(UiError),
}

#[derive(Debug)]
pub enum InitiatedEvent {
    Recovery,
    Refresh,
    Download(Box<str>),
    Scanning,
}

#[derive(Debug)]
pub enum CompletedEvent {
    Recovery,
    Refresh,
    Download,
    Scan {
        upgrade_text: Box<str>,
        upgrade:      Option<ReleaseInfo>,
        refresh:      bool,
        is_lts:       bool,
    },
}

#[derive(Debug)]
pub enum ProgressEvent {
    Fetching(u64, u64),
    Recovery(u64, u64),
    Updates(u8),
}
