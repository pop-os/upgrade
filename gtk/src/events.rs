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
    DismissNotification,
    RefreshOS,
    RepoModify(Vec<Box<str>>, Vec<bool>),
    Reset,
    Scan,
    Shutdown,
}

/// Events received for the UI to handle.
#[derive(Debug)]
pub enum UiEvent {
    Completed(CompletedEvent),
    Dismissed,
    Error(UiError),
    IncompatibleRepos(RepoCompatError),
    Initiated(InitiatedEvent),
    Progress(ProgressEvent),
    ReleaseUpgradeDialog,
    Shutdown,
    StatusChanged(DaemonStatus, DaemonStatus, Box<str>),
    UpgradeClicked,
    UpgradeEvent(UpgradeEvent),
    UpgradeNotificationClicked,
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
    Scan(ScanEvent),
}

#[derive(Debug)]
pub enum ScanEvent {
    PermissionDenied,
    Found {
        is_lts:        bool,
        refresh:       bool,
        status_failed: bool,
        reboot_ready:  bool,
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
