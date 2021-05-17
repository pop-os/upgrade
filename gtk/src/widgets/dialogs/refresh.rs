use gtk::prelude::*;

use super::DialogTemplate;
use crate::{battery, fl};

#[derive(AsRef, Deref)]
#[as_ref]
#[deref]
pub struct RefreshDialog(DialogTemplate);

impl RefreshDialog {
    pub fn new() -> Self {
        Self(cascade! {
            DialogTemplate::new(
                "view-refresh",
                &fl!("dialog-refresh-title"),
                &fl!("button-perform-refresh"),
                &gtk::STYLE_CLASS_DESTRUCTIVE_ACTION,
                |content| {
                    if battery::active() {
                        content.add(&battery_notice());
                    }

                    content.add(&refresh_description());
                },
            );
            ..set_size_request(480, 300);
            ..set_valign(gtk::Align::Start);
        })
    }
}

fn battery_notice() -> gtk::Label {
    gtk::LabelBuilder::new().label(&fl!("battery-notice")).use_markup(true).xalign(0.0).build()
}

fn refresh_description() -> gtk::Label {
    gtk::LabelBuilder::new()
        .label(&fl!("dialog-refresh-description"))
        .wrap(true)
        .xalign(0.0)
        .build()
}
