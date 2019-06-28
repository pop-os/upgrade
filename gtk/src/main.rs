#[macro_use]
extern crate cascade;

use gio::{prelude::*, ApplicationFlags};
use gtk::{prelude::*, Application};
use pop_upgrade_gtk::*;

pub const APP_ID: &str = "com.system76.UpgradeManager";

fn main() {
    glib::set_program_name(APP_ID.into());

    let application =
        Application::new(APP_ID, ApplicationFlags::empty()).expect("GTK initialization failed");

    application.connect_activate(|app| {
        if let Some(window) = app.get_window_by_id(0) {
            window.present();
        }
    });

    application.connect_startup(|app| {
        let widget = UpgradeWidget::new();
        widget.scan();

        let headerbar = cascade! {
            gtk::HeaderBar::new();
            ..set_title("Pop! Upgrade Manager");
            ..set_show_close_button(true);
            ..show();
        };

        let _window = cascade! {
            gtk::ApplicationWindow::new(app);
            ..set_titlebar(Some(&headerbar));
            ..set_icon_name("firmware-manager");
            ..set_keep_above(true);
            ..set_property_window_position(gtk::WindowPosition::Center);
            ..add(cascade! {
                widget.as_ref();
                ..set_border_width(12);
                ..set_margin_top(24);
                ..set_halign(gtk::Align::Center);
            });
            ..show();
            ..connect_delete_event(move |window, _| {
                window.destroy();

                // Allow this closure to attain ownership of our firmware widget,
                // so that this widget will exist for as long as the window exists.
                let _widget = &widget;

                Inhibit(false)
            });
        };
    });

    application.run(&[]);
}
