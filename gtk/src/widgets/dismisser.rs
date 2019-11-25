use glib::SignalHandlerId;
use gtk::prelude::*;

#[derive(Shrinkwrap)]
pub struct Dismisser {
    #[shrinkwrap(main_field)]
    container: gtk::Widget,

    pub button:    gtk::Button,
    button_signal: SignalHandlerId,
}

impl Dismisser {
    pub fn new<F: Fn() + 'static>(release: &str, dismiss_action: F) -> Self {
        let button = gtk::ButtonBuilder::new().label("Dismiss").valign(gtk::Align::Center).build();

        let title = gtk::LabelBuilder::new().label("Notifications").xalign(0.0).build();

        let label_text = &[
            "Dismiss upgrade notifications for Pop!_OS ",
            release,
            " until the next upgrade is available",
        ]
        .concat();

        let label =
            gtk::LabelBuilder::new().label(label_text).xalign(0.0).hexpand(true).wrap(true).build();

        label.get_style_context().add_class("dim-label");

        let grid = cascade! {
            gtk::Grid::new();
            ..set_column_spacing(12);
            ..set_row_spacing(4);
            ..set_margin_start(20);
            ..set_margin_top(9);
            ..set_margin_end(20);
            ..set_margin_bottom(9);
            ..attach(&title, 0, 0, 1, 1);
            ..attach(&label, 0, 1, 1, 1);
            ..attach(&button, 1, 0, 1, 2);
        };

        Self {
            button_signal: button.connect_clicked(move |button: &gtk::Button| {
                button.set_sensitive(false);
                dismiss_action();
            }),
            container: grid.upcast::<gtk::Widget>(),
            button,
        }
    }

    pub fn set_dismissed(&self, dismissed: bool) { self.button.set_sensitive(!dismissed) }
}
