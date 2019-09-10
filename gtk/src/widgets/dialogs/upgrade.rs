use gtk::prelude::*;

#[derive(Shrinkwrap)]
pub struct UpgradeDialog {
    #[shrinkwrap(main_field)]
    dialog: gtk::Dialog,
}

impl UpgradeDialog {
    pub fn new(version: &str, changelog: &str) -> Self {
        let upgrade_label = ["Pop!_OS ", version, " is available. New features include:"].concat();

        let changelog_label =
            gtk::LabelBuilder::new().label(changelog).margin_start(24).margin_end(40).build();

        let dialog = super::dialog_template(
            "distributor-logo-upgrade-symbolic",
            "Upgrade",
            "Reboot & Upgrade",
            &gtk::STYLE_CLASS_DESTRUCTIVE_ACTION,
            |content| {
                content.add(&gtk::LabelBuilder::new().label(&upgrade_label).build());
                content.add(&cascade! {
                    gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
                    ..set_hexpand(true);
                    ..set_vexpand(true);
                    ..add(&changelog_label);
                });
            },
        );

        Self { dialog }
    }
}
