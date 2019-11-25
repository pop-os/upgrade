use super::UpgradeOption;

use gtk::prelude::*;

pub struct Section {
    pub frame:  gtk::Frame,
    pub label:  gtk::Label,
    pub list:   gtk::ListBox,
    pub option: UpgradeOption,
}

impl Section {
    pub fn new(label: &str) -> Self {
        let option = UpgradeOption::new();
        let label = create_label(label);

        let list = cascade! {
            gtk::ListBox::new();
            ..set_selection_mode(gtk::SelectionMode::None);
            ..set_header_func(Some(Box::new(separator_header)));
            ..add(option.as_ref());
        };

        let frame = super::wrap_frame(list.upcast_ref::<gtk::Widget>());

        Self { frame, label, list, option }
    }

    pub fn hide(&self) {
        self.frame.hide();
        self.label.hide();
    }

    pub fn show(&self) {
        self.frame.show();
        self.label.show();
    }
}

fn create_label(label: &str) -> gtk::Label {
    gtk::LabelBuilder::new().label(label).use_markup(true).xalign(0.0).build()
}

fn separator_header(current: &gtk::ListBoxRow, _before: Option<&gtk::ListBoxRow>) {
    current.set_header(Some(&gtk::Separator::new(gtk::Orientation::Horizontal)));
}
