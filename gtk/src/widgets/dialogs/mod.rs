mod refresh;
mod upgrade;

pub use self::{refresh::RefreshDialog, upgrade::UpgradeDialog};

use crate::fl;
use gtk::prelude::*;

#[derive(AsRef, Deref)]
#[as_ref]
#[deref]
pub struct DialogTemplate(gtk::Dialog);

impl DialogTemplate {
    pub fn new<F: FnOnce(&gtk::Box)>(
        icon: &str,
        title: &str,
        accept: &str,
        accept_style: &'static str,
        func: F,
    ) -> Self {
        let cancel = gtk::Button::with_label(&fl!("button-cancel"));

        let accept = cascade! {
            gtk::Button::with_label(accept);
            ..style_context().add_class(accept_style);
            ..style_context().add_class(accept_style);
        };

        let dialog = gtk::Dialog::builder()
            .accept_focus(true)
            .deletable(true)
            .destroy_with_parent(true)
            .use_header_bar(1i32)
            .build();

        let title =
            gtk::Label::builder().label(&*["<b>", title, "</b>"].concat()).use_markup(true).build();

        cascade! {
            dialog
                .header_bar()
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

        let icon = gtk::Image::builder()
            .icon_name(icon)
            .icon_size(gtk::IconSize::Dialog)
            .valign(gtk::Align::Start)
            .build();

        cascade! {
            dialog.content_area();
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
        Self(dialog)
    }
}

impl Drop for DialogTemplate {
    fn drop(&mut self) { unsafe { self.destroy() } }
}
