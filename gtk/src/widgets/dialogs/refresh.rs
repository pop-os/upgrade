use gtk::prelude::*;

use super::DialogTemplate;
use crate::battery;

#[derive(AsRef, Deref)]
#[as_ref]
#[deref]
pub struct RefreshDialog(DialogTemplate);

impl RefreshDialog {
    pub fn new() -> Self {
        Self(cascade! {
            DialogTemplate::new(
                "view-refresh",
                "Refresh OS Install",
                "Reboot & Refresh",
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
    gtk::LabelBuilder::new()
        .label("<b>Plug into power</b> before you begin.")
        .use_markup(true)
        .xalign(0.0)
        .build()
}

fn refresh_description() -> gtk::Label {
    const DESCRIPTION: &str = r#"When you refresh the OS:

* All user accounts and files in the /home directory will be kept
* Users and user groups will be retained
* All applications installed by the user will be removed
* All files outside of the /home directory in the OS partition will be lost
* All system-wide configuration changes will be lost, with the exception of:
    - The system timezone
    - The system language
    - The system keyboard layout
    - Network configurations managed by NetworkManager

Please be sure to save all of your work before clicking to reboot."#;

    gtk::LabelBuilder::new().label(DESCRIPTION).wrap(true).xalign(0.0).build()
}
