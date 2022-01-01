use gtk::prelude::*;

pub fn scale_label(label: &gtk::Label, scale: f64) {
    let list = label.get_attributes().unwrap_or_default();
    list.insert(pango::Attribute::new_scale(scale).expect("new scale returned null"));
    label.set_attributes(Some(&list));
}
