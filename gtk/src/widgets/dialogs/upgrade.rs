use super::DialogTemplate;
use crate::battery;
use gtk::prelude::*;
use pop_upgrade::changelogs;

const CHANGELOG_PADDING: i32 = 48;

#[derive(AsRef, Deref)]
pub struct UpgradeDialog {
    #[as_ref]
    #[deref]
    dialog: DialogTemplate,
}

impl UpgradeDialog {
    pub fn new(since: &str, version: &str) -> Self {
        let title = gtk::LabelBuilder::new()
            .label(
                &["Pop!_OS ", version, " is available. ", battery_label(), "New features include:"]
                    .concat(),
            )
            .use_markup(true)
            .xalign(0.0)
            .build();
        let changelog_list = gtk::Box::new(gtk::Orientation::Vertical, 24);

        let scroller = cascade! {
            gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
            ..set_hexpand(true);
            ..set_vexpand(true);
            ..add(&changelog_list);
        };

        let mut iter = changelogs::since(since);

        match iter.next() {
            Some((version, changelog)) => {
                add_changelog(&changelog_list, changelog);
                for (version, changelog) in iter {
                    changelog_list.add(&gtk::Separator::new(gtk::Orientation::Horizontal));
                    add_version(&changelog_list, version);
                    add_changelog(&changelog_list, changelog);
                }
            }
            None => {
                add_changelog(&changelog_list, "No changelog found");
            }
        }

        let dialog = DialogTemplate::new(
            "distributor-logo",
            "Upgrade",
            "Reboot & Upgrade",
            &gtk::STYLE_CLASS_DESTRUCTIVE_ACTION,
            |content| {
                content.add(&title);
                content.add(&scroller);
            },
        );

        dialog.set_size_request(800, 600);

        Self { dialog }
    }
}

fn add_changelog(changelogs: &gtk::Box, changelog: &str) {
    let changelog_label = gtk::LabelBuilder::new()
        .label(changelog)
        .wrap(true)
        .xalign(0.0)
        .max_width_chars(40)
        .margin_start(CHANGELOG_PADDING)
        .margin_end(CHANGELOG_PADDING)
        .build();

    changelogs.add(&changelog_label);
}

fn add_version(changelogs: &gtk::Box, version: &str) {
    let upgrade_label =
        gtk::LabelBuilder::new().label(&["Pop!_OS ", version].concat()).xalign(0.0).build();
    changelogs.add(&upgrade_label);
}

fn battery_label() -> &'static str {
    if battery::active() {
        "<b>Plug into power</b> before you begin. "
    } else {
        ""
    }
}
