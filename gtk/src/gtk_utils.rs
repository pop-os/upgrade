use gtk::prelude::*;

pub fn scale_label(label: &gtk::Label, scale: f64) {
    let list = label.attributes().unwrap_or_default();
    list.insert(pango::AttrFloat::new_scale(scale));
    label.set_attributes(Some(&list));
}
