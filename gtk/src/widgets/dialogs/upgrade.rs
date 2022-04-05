use super::DialogTemplate;
use crate::{battery, fl};
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
        let title = gtk::Label::builder()
            .label(&fomat!(
                (fl!("upgrade-available", version = version)) " "
                if battery::active() {
                    (fl!("battery-notice")) " "
                }
                (fl!("new-features-include"))
            ))
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
            Some((_version, changelog)) => {
                add_changelog(&changelog_list, changelog);
                for (version, changelog) in iter {
                    changelog_list.add(&gtk::Separator::new(gtk::Orientation::Horizontal));
                    add_version(&changelog_list, version);
                    add_changelog(&changelog_list, changelog);
                }
            }
            None => {
                add_changelog(&changelog_list, &fl!("error-no-changelog-found"));
            }
        }

        let dialog = DialogTemplate::new(
            "distributor-logo",
            &fl!("button-upgrade"),
            &fl!("button-perform-upgrade"),
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
    let changelog_label = gtk::Label::builder()
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
        gtk::Label::builder().label(&["Pop!_OS ", version].concat()).xalign(0.0).build();
    changelogs.add(&upgrade_label);
}
