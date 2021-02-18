use super::DialogTemplate;
use gtk::prelude::*;

#[derive(AsRef, Deref)]
pub struct RepositoryDialog {
    #[as_ref]
    #[deref]
    dialog: DialogTemplate,
}

impl RepositoryDialog {
    pub fn new<S: AsRef<str>>(repositories: impl Iterator<Item = S>) -> Self {
        let entries = cascade! {
            let list = gtk::ListBox::new();
            ..set_selection_mode(gtk::SelectionMode::None);
            for repository in repositories {
                list.insert(&gtk::CheckButton::with_label(repository.as_ref()), -1);
            };
        };

        let dialog = DialogTemplate::new(
            "application-x-deb",
            "3rd party repositories",
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

        Self { dialog }
    }
}
