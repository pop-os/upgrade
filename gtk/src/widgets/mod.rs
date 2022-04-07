pub mod dialogs;
pub mod permissions;

mod dismisser;
mod section;
mod upgrade_option;

pub use self::{dismisser::Dismisser, section::Section, upgrade_option::UpgradeOption};

use gtk::prelude::*;

pub fn wrap_frame(widget: &gtk::Widget) -> gtk::Frame {
    let frame = cascade! {
        gtk::Frame::new(None);
        ..set_margin_bottom(12);
        ..add(widget);
        ..show_all();
    };

    frame
}
