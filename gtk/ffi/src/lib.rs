use glib::object::ObjectType;
use pop_upgrade_gtk::*;
use std::{ffi, ptr};

#[no_mangle]
pub struct PopUpgradeWidget;

pub type PopUpgradeWidgetErrorCallback =
    extern "C" fn(message: *const u8, message_len: usize, user_data: *mut ffi::c_void);

pub type PopUpgradeWidgetEventCallback = extern "C" fn(event: u8, user_data: *mut ffi::c_void);

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
pub extern "C" fn pop_upgrade_widget_callback_error(
    ptr: *const PopUpgradeWidget,
    callback: PopUpgradeWidgetErrorCallback,
    user_data: *mut ffi::c_void,
) {
    if let Some(widget) = unsafe { (ptr as *const UpgradeWidget).as_ref() } {
        widget.callback_error(move |message| {
            callback(message.as_bytes().as_ptr(), message.len(), user_data)
        });
    }
}

#[no_mangle]
pub extern "C" fn pop_upgrade_widget_callback_event(
    ptr: *const PopUpgradeWidget,
    callback: PopUpgradeWidgetEventCallback,
    user_data: *mut ffi::c_void,
) {
    if let Some(widget) = unsafe { (ptr as *const UpgradeWidget).as_ref() } {
        widget.callback_event(move |event| callback(event as u8, user_data));
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
    let widget = unsafe { Box::from_raw(widget as *mut UpgradeWidget) };
    widget.shutdown();
}
