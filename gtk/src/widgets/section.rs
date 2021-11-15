use super::UpgradeOption;

use gtk::prelude::*;

pub struct Section {
    pub frame: gtk::Frame,
    pub label: gtk::Label,
    pub list:  gtk::ListBox,
}

impl Section {
    pub fn new(label: &str) -> Self {
        let label = create_label(label);

        let list = cascade! {
            gtk::ListBox::new();
            ..set_selection_mode(gtk::SelectionMode::None);
            ..set_header_func(Some(Box::new(separator_header)));
        };

        let frame = super::wrap_frame(list.upcast_ref::<gtk::Widget>());

        Self { frame, label, list }
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
    gtk::Label::builder().label(label).use_markup(true).xalign(0.0).build()
}

fn separator_header(current: &gtk::ListBoxRow, _before: Option<&gtk::ListBoxRow>) {
    current.set_header(Some(&gtk::Separator::new(gtk::Orientation::Horizontal)));
}

/// A section specialized for upgrade and recovery options.
#[derive(Deref, DerefMut)]
pub struct UpgradeSection {
    #[deref]
    #[deref_mut]
    section: Section,

    pub options: Vec<UpgradeOption>,
}

impl UpgradeSection {
    pub fn new(label: &str) -> Self { Self { section: Section::new(label), options: Vec::new() } }

    pub fn add_option(&mut self, option: UpgradeOption) {
        self.list.add(option.as_ref());
        self.options.push(option);
    }

    pub fn disable(&self, indice: usize, message: &str) {
        self.options[indice].label(message).button.hide();
    }
}
