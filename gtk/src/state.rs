use crate::{events::*, widgets::Dismisser, ErrorCallback, EventCallback, ReadyCallback};

use pop_upgrade::client::ReleaseInfo;

use std::sync::{self, mpsc::SyncSender};

#[derive(new)]
pub struct State {
    #[new(default)]
    pub fetching_release: bool,
    #[new(default)]
    pub refresh_found: bool,
    #[new(default)]
    pub upgrade_downloaded: bool,
    #[new(default)]
    pub upgrade_found: bool,

    #[new(default)]
    pub upgrade_label: Box<str>,
    #[new(default)]
    pub upgrading_from: Box<str>,
    #[new(default)]
    pub upgrading_to: Box<str>,

    #[new(default)]
    pub dismisser: Option<Dismisser>,
    #[new(default)]
    pub upgrade_version: Option<ReleaseInfo>,

    pub sender:     SyncSender<BackgroundEvent>,
    pub gui_sender: sync::Weak<glib::Sender<UiEvent>>,

    pub callback_error: ErrorCallback,
    pub callback_event: EventCallback,
    pub callback_ready: ReadyCallback,
}

impl State {
    pub fn calculate_fetching_progress(&self, mut progress: u64, total: u64) -> u64 {
        progress = if self.fetching_release { progress / 2 } else { progress / 4 };
        progress = progress * 100 / total;

        if self.fetching_release {
            progress += 50;
        }

        progress
    }
}
