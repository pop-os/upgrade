use glib::object::ObjectType;
use pop_upgrade_gtk::*;
use std::ptr;

#[no_mangle]
pub struct PopUpgradeWidget;

#[no_mangle]
pub extern "C" fn pop_upgrade_widget_new() -> *mut PopUpgradeWidget {
    // When used from C, assume that GTK has been initialized.
    unsafe {
        gtk::set_initialized();
    }

    UpgradeWidget::new()
        .map_err(|why| eprintln!("failed to create upgrade widget: {}", why))
        .map(Box::new)
        .map(Box::into_raw)
        .unwrap_or(ptr::null_mut()) as *mut _
}

#[no_mangle]
pub extern "C" fn pop_upgrade_widget_scan(ptr: *mut PopUpgradeWidget) -> i32 {
    let widget = unsafe { (ptr as *mut UpgradeWidget).as_mut() };
    widget.map_or(-1, |widget| match widget.scan() {
        Ok(_) => 0,
        Err(why) => {
            eprintln!("failed to get upgrade options: {}", why);
            -1
        }
    })
}

#[no_mangle]
pub extern "C" fn pop_upgrade_widget_container(
    ptr: *const PopUpgradeWidget,
) -> *mut gtk_sys::GtkContainer {
    let value = unsafe { (ptr as *const UpgradeWidget).as_ref() };
    value.map_or(ptr::null_mut(), |widget| widget.container().as_ptr())
}

#[no_mangle]
pub extern "C" fn pop_upgrade_widget_free(widget: *mut PopUpgradeWidget) {
    unsafe { Box::from_raw(widget as *mut UpgradeWidget) };
}
