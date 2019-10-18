use glib::SignalHandlerId;
use gtk::prelude::*;
use std::cell::RefCell;

#[derive(Shrinkwrap)]
pub struct UpgradeOption {
    #[shrinkwrap(main_field)]
    container: gtk::Container,
    pub button: gtk::Button,
    button_signal: RefCell<Option<SignalHandlerId>>,
    label: gtk::Label,
    progress: gtk::ProgressBar,
    pub stack: gtk::Stack,
    sublabel: gtk::Label,
}

impl UpgradeOption {
    pub fn new() -> Self {
        let button = cascade! {
            gtk::Button::new_with_label("");
            ..set_hexpand(true);
            ..set_halign(gtk::Align::End);
            ..set_can_focus(true);
            ..get_style_context().add_class(&gtk::STYLE_CLASS_SUGGESTED_ACTION);
        };

        let label = cascade! {
            gtk::Label::new(None);
            ..set_xalign(0.0);
            ..set_vexpand(true);
            ..set_mnemonic_widget(Some(&button));
        };

        let sublabel = cascade! {
            gtk::Label::new(None);
            ..set_xalign(0.0);
            ..get_style_context().add_class(&gtk::STYLE_CLASS_DIM_LABEL);
            ..set_no_show_all(true);
        };

        let progress = cascade! {
            gtk::ProgressBar::new();
            ..set_ellipsize(pango::EllipsizeMode::End);
            ..set_hexpand(true);
            ..set_valign(gtk::Align::End);
        };

        let stack = cascade! {
            gtk::Stack::new();
            ..add(&button);
            ..add(&progress);
            ..set_visible_child(&button);
            ..show_all();
        };

        let container = cascade! {
            gtk::Grid::new();
            ..set_margin_start(20);
            ..set_margin_end(20);
            ..set_margin_top(9);
            ..set_margin_bottom(9);
            ..set_column_spacing(12);
            ..attach(&label,    0, 0, 1, 1);
            ..attach(&sublabel, 0, 1, 1, 1);
            ..attach(&stack,   1, 0, 1, 2);
            ..show_all();
        };

        Self {
            button_signal: RefCell::new(None),
            button,
            container: container.upcast::<gtk::Container>(),
            label,
            progress,
            stack,
            sublabel,
        }
    }

    pub fn button_view(&self) -> &Self {
        self.stack.set_visible_child(&self.button);
        self
    }

    pub fn button_label(&self, label: &str) -> &Self {
        self.button.set_label(label);
        self.show_all();
        self
    }

    pub fn progress(&self, current: u64, total: u64) -> &Self {
        self.progress.set_fraction(current as f64 / total as f64);
        self
    }

    pub fn progress_exact(&self, percent: u8) -> &Self {
        self.progress.set_fraction(percent as f64 / 100f64);
        self
    }

    pub fn set_label(&self, label: &str) -> &Self {
        self.label.set_label(label);
        self
    }

    pub fn set_sublabel(&self, label: Option<&str>) -> &Self {
        match label {
            Some(label) => {
                self.sublabel.set_label(label);
                self.sublabel.show();
            }
            None => self.sublabel.hide(),
        }

        self
    }

    pub fn set_button<F: Fn() + 'static>(&self, action: Option<(&str, F)>) -> &Self {
        let mut button_signal = self.button_signal.borrow_mut();

        if let Some(id) = button_signal.take() {
            glib::signal_handler_disconnect(&self.button, id);
        }

        match action {
            Some((label, func)) => {
                self.button.set_label(label);
                self.button.set_visible(true);
                let id = self.button.connect_clicked(move |button| {
                    button.hide();
                    func()
                });
                *button_signal = Some(id);
            }
            None => self.button.set_visible(false),
        }

        self
    }

    fn progress_view(&self) -> &Self {
        self.stack.set_visible_child(&self.progress);
        self
    }
}
