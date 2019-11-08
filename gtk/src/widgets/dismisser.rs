use glib::SignalHandlerId;
use gtk::prelude::*;

#[derive(Shrinkwrap)]
pub struct Dismisser {
    #[shrinkwrap(main_field)]
    container: gtk::Widget,

    button:        gtk::Switch,
    button_signal: SignalHandlerId,
}

impl Dismisser {
    pub fn new<F: Fn(bool) + 'static>(release: &str, dismiss_action: F) -> Self {
        let button = gtk::SwitchBuilder::new().halign(gtk::Align::End).build();

        let dismiss_action = move |button: &gtk::Switch| dismiss_action(!button.get_active());

        let label = gtk::LabelBuilder::new()
            .label(&["Receive upgrade notifications for Pop!_OS ", release].concat())
            .xalign(0.0)
            .hexpand(true)
            .wrap(true)
            .build();

        let container = cascade! {
            gtk::Box::new(gtk::Orientation::Horizontal, 12);
            ..set_margin_start(20);
            ..set_margin_end(20);
            ..add(&label);
            ..add(&button);
        };

        Self {
            button_signal: button.connect_changed_active(dismiss_action),
            container: container.upcast_ref::<gtk::Widget>().clone(),
            button,
        }
    }

    pub fn set_dismissed(&self, dismissed: bool) {
        self.button.block_signal(&self.button_signal);
        self.button.set_active(!dismissed);
        self.button.unblock_signal(&self.button_signal);
    }
}
