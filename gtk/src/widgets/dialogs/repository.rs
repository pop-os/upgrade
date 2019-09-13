use gtk::prelude::*;

#[derive(Shrinkwrap)]
pub struct RepositoryDialog {
    #[shrinkwrap(main_field)]
    dialog: gtk::Dialog,
    entries: gtk::ListBox,
}

impl RepositoryDialog {
    pub fn new<S: AsRef<str>>(repositories: impl Iterator<Item = S>) -> Self {
        let entries = cascade! {
            list: gtk::ListBox::new();
            ..set_selection_mode(gtk::SelectionMode::None);
            | for repository in repositories {
                list.insert(&gtk::CheckButton::new_with_label(repository.as_ref()), -1);
            };
        };

        let dialog = super::dialog_template(
            "application-x-deb",
            "Unsupported repositories detected",
            "Accept",
            &gtk::STYLE_CLASS_SUGGESTED_ACTION,
            |content| {
                content.add(
                    &gtk::LabelBuilder::new().label("Select which repositories to keep.").build(),
                );
                content.add(&cascade! {
                    gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
                    ..set_hexpand(true);
                    ..set_vexpand(true);
                    ..add(&entries);
                });
            },
        );

        dialog.set_size_request(600, 400);

        Self { dialog, entries }
    }

    pub fn answers(&self) -> impl Iterator<Item = bool> {
        self.entries
            .get_children()
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::ListBoxRow>().ok())
            .flat_map(|w| w.get_children().into_iter())
            .filter_map(|w| w.downcast::<gtk::CheckButton>().ok())
            .map(|w| w.get_active())
    }
}
