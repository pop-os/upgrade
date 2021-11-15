pub mod dialogs;
pub mod permissions;

mod dismisser;
mod section;
mod upgrade_option;

pub use self::{dismisser::Dismisser, section::*, upgrade_option::UpgradeOption};

use gtk::prelude::*;

pub fn option_container() -> gtk::Grid {
    gtk::Grid::builder()
        .margin_start(20)
        .margin_end(20)
        .margin_top(8)
        .margin_bottom(8)
        .column_spacing(24)
        .row_spacing(4)
        .width_request(-1)
        .height_request(32)
        .build()
}

pub fn wrap_frame(widget: &gtk::Widget) -> gtk::Frame {
    cascade! {
        gtk::Frame::new(None);
        ..set_margin_bottom(12);
        ..add(widget);
        ..show_all();
    }
}
