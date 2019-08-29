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

        let cancel = gtk::Button::new_with_label("Cancel".into());

        let accept = cascade! {
            gtk::Button::new_with_label("Accept".into());
            ..get_style_context().add_class(&gtk::STYLE_CLASS_SUGGESTED_ACTION);
        };

        let dialog = gtk::DialogBuilder::new()
            .accept_focus(true)
            .deletable(true)
            .destroy_with_parent(true)
            .use_header_bar(1i32)
            .build();

        cascade! {
            dialog
                .get_header_bar()
                .expect("dialog generated without header bar")
                .downcast::<gtk::HeaderBar>()
                .expect("dialog header bar is not a header bar");
            ..set_custom_title(Some(&gtk::Label::new(Some("Unsupported repositories detected"))));
            ..set_show_close_button(false);
            ..pack_end(&accept);
            ..pack_start(&cancel);
        };

        cascade! {
            dialog.get_content_area();
            ..set_orientation(gtk::Orientation::Horizontal);
            ..set_border_width(12);
            ..set_spacing(12);
            ..add(
                &gtk::ImageBuilder::new()
                    .icon_name("application-x-deb")
                    .icon_size(gtk::IconSize::Dialog.into())
                    .valign(gtk::Align::Start)
                    .build()
            );
            ..add(&cascade! {
                gtk::Box::new(gtk::Orientation::Vertical, 12);
                ..set_hexpand(true);
                ..set_vexpand(true);
                ..add(&gtk::LabelBuilder::new().label("Select which repositories to keep.").build());
                ..add(&cascade! {
                    gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
                    ..set_hexpand(true);
                    ..set_vexpand(true);
                    ..add(&entries);
                });
            });
        };

        {
            let dialog = dialog.clone();
            cancel.connect_clicked(move |_| {
                dialog.response(gtk::ResponseType::Cancel);
            });
        }

        {
            let dialog = dialog.clone();
            accept.connect_clicked(move |_| {
                dialog.response(gtk::ResponseType::Accept);
            });
        }

        Self { dialog, entries }
    }

    pub fn answers(&self) -> impl Iterator<Item = bool> {
        self.entries
            .get_children()
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::ToggleButton>().ok())
            .map(|w| w.get_active())
    }
}
