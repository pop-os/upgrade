use crate::fl;
use gtk::prelude::*;

#[derive(Shrinkwrap)]
pub struct PermissionDenied(gtk::Container);

impl PermissionDenied {
    pub fn new() -> Self {
        let container = cascade! {
            gtk::Box::new(gtk::Orientation::Horizontal, 24);
            ..set_halign(gtk::Align::Center);
            ..set_valign(gtk::Align::Center);
            ..add(
                &gtk::Image::builder()
                    .icon_name("system-lock-screen-symbolic")
                    .icon_size(gtk::IconSize::Dialog)
                    .pixel_size(64)
                    .build()
            );
            ..add(&cascade! {
                gtk::Label::builder()
                    .label(&fl!("permission-denied"))
                    .wrap(true)
                    .xalign(0.0)
                    .yalign(0.0)
                    .build();
                ..style_context().add_class(&gtk::STYLE_CLASS_DIM_LABEL);
            });
            ..show_all();
        };

        Self(container.upcast::<gtk::Container>())
    }
}
