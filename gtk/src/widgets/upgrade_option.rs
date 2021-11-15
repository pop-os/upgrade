use crate::gtk_utils::scale_label;
use glib::SignalHandlerId;
use gtk::prelude::*;
use std::cell::RefCell;

#[derive(Shrinkwrap)]
pub struct UpgradeOption {
    #[shrinkwrap(main_field)]
    container: gtk::Grid,

    pub button:   gtk::Button,
    pub label:    gtk::Label,
    pub progress: gtk::ProgressBar,
    pub sublabel: gtk::Label,

    button_label: gtk::Label,
    last_class:   Option<&'static str>,

    button_signal: RefCell<Option<SignalHandlerId>>,
}

impl UpgradeOption {
    pub fn new() -> Self {
        let button_label = gtk::Label::builder().margin(4).build();

        let button = cascade! {
            gtk::Button::builder()
                .can_focus(true)
                .halign(gtk::Align::End)
                .hexpand(true)
                .valign(gtk::Align::Center)
                .build();
            ..add(&button_label);
        };

        let label = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .vexpand(true)
            .mnemonic_widget(&button)
            .build();

        let sublabel = cascade! {
            let label = gtk::Label::new(None);
            ..set_line_wrap(true);
            ..set_hexpand(true);
            ..set_xalign(0.0);
            ..set_yalign(0.0);
            ..style_context().add_class(&gtk::STYLE_CLASS_DIM_LABEL);
            ..set_no_show_all(true);
            ..hide();
            scale_label(&label, 0.9);
        };

        let labels = cascade! {
            gtk::Box::new(gtk::Orientation::Vertical, 4);
            ..add(&label);
            ..add(&sublabel);
        };

        let progress = cascade! {
            gtk::ProgressBar::new();
            ..set_ellipsize(pango::EllipsizeMode::End);
            ..set_hexpand(true);
            ..set_no_show_all(true);
            ..hide();
        };

        let container = cascade! {
            super::option_container();
            ..attach(&labels,   0, 0, 1, 1);
            ..attach(&button,   1, 0, 1, 1);
            ..attach(&progress, 0, 1, 2, 1);
            ..show_all();
        };

        Self {
            button_label,
            button_signal: RefCell::new(None),
            button,
            container,
            label,
            last_class: None,
            progress,
            sublabel,
        }
    }

    pub fn button_class(&mut self, class: &'static str) -> &Self {
        let ctx = self.button.style_context();

        if let Some(class) = self.last_class {
            ctx.remove_class(class);
        }

        ctx.add_class(class);
        self.last_class = Some(class);

        self
    }

    /// Sets the button label
    pub fn button_label(&self, label: &str) -> &Self {
        self.button_label.set_text(label);
        self
    }

    /// Programs the click signal of the button.
    ///
    /// This automatically hides the button on click.
    pub fn button_signal<F: Fn() + 'static>(&self, action: Option<(String, F)>) -> &Self {
        let mut button_signal = self.button_signal.borrow_mut();

        if let Some(id) = button_signal.take() {
            glib::signal_handler_disconnect(&self.button, id);
        }

        match action {
            Some((label, func)) => {
                self.button_label(&label);
                self.show_button();
                let id = self.button.connect_clicked(move |button| {
                    button.hide();
                    func();
                });
                *button_signal = Some(id);
            }
            None => self.button.hide(),
        }

        self
    }

    pub fn sensitive(&self, sensitive: bool) -> &Self {
        self.button.set_sensitive(sensitive);
        self
    }

    /// Set the label describing the option to be applied, or the status of the operation.
    pub fn label(&self, label: &str) -> &Self {
        self.label.set_label(label);
        self
    }

    /// Hide the progress bar and button
    pub fn hide_widgets(&self) -> &Self {
        self.button.hide();
        self.progress.hide();
        self
    }

    /// Calculate the progress bar percent based on the current and total.
    pub fn progress(&self, current: u64, total: u64) -> &Self {
        self.progress_exact((current * 100 / total) as u8)
    }

    /// Set the progress bar to the exact percent as defined.
    pub fn progress_exact(&self, percent: u8) -> &Self {
        // Only set if the new progress is higher than the current.
        let new = f64::from(percent) / 100f64;
        if new > self.progress.fraction() {
            self.progress.set_fraction(new);
        }

        self
    }

    /// Reset the progress bar % to 0.
    pub fn reset_progress(&self) -> &Self {
        self.progress.set_fraction(0f64);
        self
    }

    /// Show the button, and hide the progress bar.
    pub fn show_button(&self) -> &Self {
        self.button.show();
        self.progress.hide();
        self
    }

    /// Show the progress bar, and hide the button.
    pub fn show_progress(&self) -> &Self {
        self.button.hide();
        self.progress.show();
        self
    }

    /// Sets a sublabel with additional information about the operation.
    pub fn sublabel(&self, label: Option<&str>) -> &Self {
        if let Some(label) = label {
            self.label.set_yalign(1.0);
            self.sublabel.set_label(label);
            self.sublabel.show();
        } else {
            self.label.set_yalign(0.5);
            self.sublabel.hide();
        }

        self
    }
}
