use crate::fl;
use gtk::prelude::*;

#[derive(Shrinkwrap)]
pub struct Dismisser {
    #[shrinkwrap(main_field)]
    container: gtk::Widget,

    pub button: gtk::Button,
}

impl Dismisser {
    pub fn new<F: Fn() + 'static>(release: &str, dismiss_action: F) -> Self {
        let button =
            gtk::Button::builder().label(&fl!("button-dismiss")).valign(gtk::Align::Center).build();

        button.connect_clicked(move |button| {
            button.set_sensitive(false);
            dismiss_action();
        });

        let title =
            gtk::Label::builder().label(&fl!("notification-dismiss-label")).xalign(0.0).build();

        let label_text = fl!("notification-dismiss-description", version = release);
        let label =
            gtk::Label::builder().label(&label_text).xalign(0.0).hexpand(true).wrap(true).build();

        label.style_context().add_class("dim-label");

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

        Self { container: grid.upcast::<gtk::Widget>(), button }
    }

    pub fn set_dismissed(&self, dismissed: bool) { self.button.set_sensitive(!dismissed) }
}
