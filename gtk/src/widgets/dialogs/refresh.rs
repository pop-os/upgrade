use gtk::prelude::*;

use super::DialogTemplate;

#[derive(AsRef, Deref)]
#[as_ref]
#[deref]
pub struct RefreshDialog(DialogTemplate);

impl RefreshDialog {
    pub fn new() -> Self {
        let description = gtk::LabelBuilder::new()
            .label("Reinstall while retaining user accounts and files")
            .xalign(0.0)
            .build();

        Self(cascade! {
            DialogTemplate::new(
                "view-refresh",
                "Refresh OS Install",
                "Reboot & Refresh",
                &gtk::STYLE_CLASS_DESTRUCTIVE_ACTION,
                |content| {
                    content.add(&description);
                },
            );
            ..set_size_request(480, 200);
        })
    }
}
