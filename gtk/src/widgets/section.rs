use super::UpgradeOption;

use gtk::prelude::*;

pub struct Section {
    pub frame:   gtk::Frame,
    pub label:   gtk::Label,
    pub list:    gtk::ListBox,
    pub options: Vec<UpgradeOption>,
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

        Self { frame, label, list, options: Vec::new() }
    }

    pub fn add_option<F: FnOnce(&mut UpgradeOption)>(
        &mut self,
        option_sg: &gtk::SizeGroup,
        button_sg: &gtk::SizeGroup,
        sublab_sg: &gtk::SizeGroup,
        func: F,
    ) -> &mut Self {
        let mut option = UpgradeOption::new();
        option_sg.add_widget(option.as_ref());
        button_sg.add_widget(&option.button);
        sublab_sg.add_widget(&option.sublabel);

        func(&mut option);

        self.list.add(option.as_ref());
        self.options.push(option);
        self
    }

    pub fn disable(&self, indice: usize, message: &str) {
        self.options[indice].label(message).button.hide();
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
