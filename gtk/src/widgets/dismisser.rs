use gtk::prelude::*;

#[derive(Shrinkwrap)]
pub struct Dismisser {
    #[shrinkwrap(main_field)]
    container: gtk::Container,

    pub button: gtk::Button,

    label: gtk::Label,
}

impl Dismisser {
    pub fn new<F: Fn() + 'static>(release: &str, dismiss_action: F) -> Self {
        let label = gtk::LabelBuilder::new()
            .label(&["Dismiss notifications for Pop!_OS ", release].concat())
            .hexpand(true)
            .halign(gtk::Align::End)
            .build();

        let button = gtk::ButtonBuilder::new().label("Dismiss").build();

        let container = cascade! {
            gtk::Box::new(gtk::Orientation::Horizontal, 12);
            ..add(&label);
            ..add(&button);
        };

        button.connect_clicked(move |_| dismiss_action());

        Self { container: container.upcast::<gtk::Container>(), button, label }
    }
}
