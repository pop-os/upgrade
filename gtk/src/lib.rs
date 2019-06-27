#[macro_use]
extern crate cascade;
#[macro_use]
extern crate shrinkwraprs;

mod widgets;

use self::widgets::*;
use gtk::prelude::*;
use pop_upgrade::{
    client::{Client, Error},
    release,
};
use std::{borrow::Cow, path::Path, rc::Rc};

#[derive(Shrinkwrap)]
pub struct UpgradeWidget(Rc<InnerWidget>);

pub struct InnerWidget {
    container: gtk::Container,
    options: gtk::ListBox,
    option_upgrade: UpgradeOption,
    option_refresh: UpgradeOption,
    client: Client,
}

impl InnerWidget {
    pub fn refresh_os(&self) {
        eprintln!("refreshing OS");
    }

    pub fn upgrade_release(&self) {
        eprintln!("upgrading release");
    }
}

impl UpgradeWidget {
    pub fn new() -> Result<Self, Error> {
        let client = Client::new()?;

        let option_upgrade = UpgradeOption::new();
        let option_refresh = UpgradeOption::new();

        cascade! {
            gtk::SizeGroup::new(gtk::SizeGroupMode::Both);
            ..add_widget(&option_upgrade.button);
            ..add_widget(&option_refresh.button);
        }

        cascade! {
            gtk::SizeGroup::new(gtk::SizeGroupMode::Both);
            ..add_widget(option_upgrade.as_ref());
            ..add_widget(option_refresh.as_ref());
        }

        option_refresh
            .set_label("Refresh OS")
            .set_sublabel("Reinstall while keeping user accounts and files".into());

        let options = cascade! {
            gtk::ListBox::new();
            ..set_selection_mode(gtk::SelectionMode::None);
            ..insert(option_upgrade.as_ref(), -1);
            ..insert(option_refresh.as_ref(), -1);
            ..show();
        };

        let container = cascade! {
            gtk::Box::new(gtk::Orientation::Vertical, 12);
            ..add(&cascade! {
                gtk::Label::new("<b>OS Upgrade &amp; Refresh</b>");
                ..set_use_markup(true);
                ..set_xalign(0.0);
                ..show();
            });
            ..add(&cascade! {
                gtk::Frame::new(None);
                ..add(&options);
                ..show();
            });
            ..show();
        };

        Ok(Self(Rc::new(InnerWidget {
            container: container.upcast::<gtk::Container>(),
            options,
            option_upgrade,
            option_refresh,
            client,
        })))
    }

    pub fn container(&self) -> &gtk::Container {
        &self.as_ref().container
    }

    fn recovery_exists(&self) -> bool {
        let exists = || Path::new("/recovery").exists();
        exists() || (self.client.release_repair().is_ok() && exists())
    }

    pub fn scan_options(&self) -> Result<(), Error> {
        self.container.hide();
        self.option_refresh.hide();

        let mut upgrade_text = Cow::Borrowed("No upgrades available");
        let mut upgrade = false;

        if release::upgrade_in_progress() {
            upgrade_text = Cow::Borrowed("Release upgrade already occuring");
        } else {
            let info = self.client.release_check()?;
            if info.build > 0 {
                eprintln!(
                    "upgrade from {} to {} is available",
                    info.current, info.current
                );

                upgrade_text =
                    Cow::Owned(format!("Upgrade from {} to {}", info.current, info.current));
                upgrade = true;
            }
        }

        eprintln!("upgrade option: {}", upgrade_text);

        self.option_upgrade
            .set_label(&upgrade_text)
            .set_sublabel(None)
            .set_button(if upgrade {
                let widget = Rc::downgrade(&self);
                let action = move || {
                    if let Some(widget) = widget.upgrade() {
                        widget.upgrade_release();
                    }
                };
                Some(("", action))
            } else {
                None
            })
            .show();

        if self.recovery_exists() {
            let widget = Rc::downgrade(&self);
            let action = move || {
                if let Some(widget) = widget.upgrade() {
                    widget.refresh_os();
                }
            };

            self.option_refresh
                .set_button(Some(("Refresh", action)))
                .show();
        }

        self.container.show();

        Ok(())
    }

    pub fn upgrade_daemon_is_active(&self) -> bool {
        self.client.status().is_ok()
    }
}
