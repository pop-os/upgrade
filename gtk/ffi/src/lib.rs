use glib::object::ObjectType;
use i18n_embed::DesktopLanguageRequester;
use pop_upgrade_gtk::{localizer, UpgradeWidget};
use std::{ffi, ptr};

pub struct PopUpgradeWidget;

pub type PopUpgradeWidgetErrorCallback =
    extern "C" fn(message: *const u8, message_len: usize, user_data: *mut ffi::c_void);

pub type PopUpgradeWidgetEventCallback = extern "C" fn(event: u8, user_data: *mut ffi::c_void);

pub type PopUpgradeWidgetReadyCallback = extern "C" fn(user_data: *const ffi::c_void);

#[no_mangle]
pub extern "C" fn pop_upgrade_widget_new() -> *mut PopUpgradeWidget {
    // When used from C, assume that GTK has been initialized.
    unsafe {
        gtk::set_initialized();
    }

    let localizer = localizer();
    let requested_languages = DesktopLanguageRequester::requested_languages();

    if let Err(error) = localizer.select(&requested_languages) {
        eprintln!("Error while loading languages for pop_upgrade_gtk {}", error);
    }

    Box::into_raw(Box::new(UpgradeWidget::new())).cast()
}

#[no_mangle]
pub extern "C" fn pop_upgrade_widget_scan(ptr: *mut PopUpgradeWidget) {
    if let Some(widget) = unsafe { (ptr.cast::<UpgradeWidget>()).as_mut() } {
        widget.scan();
    }
}

#[no_mangle]
pub extern "C" fn pop_upgrade_widget_callback_error(
    ptr: *const PopUpgradeWidget,
    callback: PopUpgradeWidgetErrorCallback,
    user_data: *mut ffi::c_void,
) {
    if let Some(widget) = unsafe { (ptr.cast::<UpgradeWidget>()).as_ref() } {
        widget.callback_error(move |message| {
            callback(message.as_bytes().as_ptr(), message.len(), user_data);
        });
    }
}

#[no_mangle]
pub extern "C" fn pop_upgrade_widget_callback_event(
    ptr: *const PopUpgradeWidget,
    callback: PopUpgradeWidgetEventCallback,
    user_data: *mut ffi::c_void,
) {
    if let Some(widget) = unsafe { (ptr.cast::<UpgradeWidget>()).as_ref() } {
        widget.callback_event(move |event| callback(event as u8, user_data));
    }
}

#[no_mangle]
pub extern "C" fn pop_upgrade_widget_callback_ready(
    ptr: *const PopUpgradeWidget,
    callback: PopUpgradeWidgetReadyCallback,
    user_data: *mut ffi::c_void,
) {
    if let Some(widget) = unsafe { (ptr.cast::<UpgradeWidget>()).as_ref() } {
        widget.callback_ready(move || callback(user_data));
    }
}

#[no_mangle]
pub extern "C" fn pop_upgrade_widget_container(
    ptr: *const PopUpgradeWidget,
) -> *mut gtk_sys::GtkContainer {
    let value = unsafe { (ptr.cast::<UpgradeWidget>()).as_ref() };
    value.map_or(ptr::null_mut(), |widget| widget.as_ref().as_ptr())
}

#[no_mangle]
pub extern "C" fn pop_upgrade_widget_free(widget: *mut PopUpgradeWidget) {
    let widget = unsafe { Box::from_raw(widget.cast::<UpgradeWidget>()) };
    widget.shutdown();
}
