mod repository;
mod upgrade;

pub use self::{repository::RepositoryDialog, upgrade::UpgradeDialog};

use gtk::prelude::*;

fn dialog_template<F: FnOnce(&gtk::Box)>(
    icon: &str,
    title: &str,
    accept: &str,
    accept_style: &'static str,
    func: F,
) -> gtk::Dialog {
    let cancel = gtk::Button::new_with_label("Cancel".into());

    let accept = cascade! {
        gtk::Button::new_with_label(accept);
        ..get_style_context().add_class(&accept_style);
    };

    let dialog = gtk::DialogBuilder::new()
        .accept_focus(true)
        .deletable(true)
        .destroy_with_parent(true)
        .use_header_bar(1i32)
        .build();

    let title =
        gtk::LabelBuilder::new().label(&*["<b>", title, "</b>"].concat()).use_markup(true).build();

    cascade! {
        dialog
            .get_header_bar()
            .expect("dialog generated without header bar")
            .downcast::<gtk::HeaderBar>()
            .expect("dialog header bar is not a header bar");
        ..set_custom_title(Some(&title));
        ..set_show_close_button(false);
        ..pack_end(&accept);
        ..pack_start(&cancel);
    };

    let content = cascade! {
        gtk::Box::new(gtk::Orientation::Vertical, 12);
        ..set_hexpand(true);
        ..set_vexpand(true);
    };

    let icon = gtk::ImageBuilder::new()
        .icon_name(icon)
        .icon_size(gtk::IconSize::Dialog.into())
        .valign(gtk::Align::Start)
        .build();

    cascade! {
        dialog.get_content_area();
        ..set_orientation(gtk::Orientation::Horizontal);
        ..set_border_width(12);
        ..set_spacing(12);
        ..add(&icon);
        ..add(&content);
    };

    func(&content);

    {
        let dialog = dialog.downgrade();
        cancel.connect_clicked(move |_| {
            if let Some(dialog) = dialog.upgrade() {
                dialog.response(gtk::ResponseType::Cancel);
            }
        });
    }

    {
        let dialog = dialog.downgrade();
        accept.connect_clicked(move |_| {
            if let Some(dialog) = dialog.upgrade() {
                dialog.response(gtk::ResponseType::Accept);
            }
        });
    }

    dialog.show_all();

    dialog
}
