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

    Box::into_raw(Box::new(UpgradeWidget::new())) as *mut _
}

#[no_mangle]
pub extern "C" fn pop_upgrade_widget_scan(ptr: *mut PopUpgradeWidget) {
    if let Some(widget) = unsafe { (ptr as *mut UpgradeWidget).as_mut() } {
        widget.scan();
    }
}

#[no_mangle]
pub extern "C" fn pop_upgrade_widget_container(
    ptr: *const PopUpgradeWidget,
) -> *mut gtk_sys::GtkContainer {
    let value = unsafe { (ptr as *const UpgradeWidget).as_ref() };
    value.map_or(ptr::null_mut(), |widget| widget.as_ref().as_ptr())
}

#[no_mangle]
pub extern "C" fn pop_upgrade_widget_free(widget: *mut PopUpgradeWidget) {
    unsafe { Box::from_raw(widget as *mut UpgradeWidget) };
}
