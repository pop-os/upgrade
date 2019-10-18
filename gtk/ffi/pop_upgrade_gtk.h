#include <gtk/gtk.h>

typedef struct { } PopUpgradeWidget;

typedef void (*PopUpgradeWidgetErrorCallback)(const uint8_t*, size_t, void*);

typedef void (*PopUpgradeWidgetEventCallback)(uint8_t, void*);

typedef void (*PopUpgradeWidgetReadyCallback)(void*);

PopUpgradeWidget *pop_upgrade_widget_new (void);

/// Triggered when an error occurs in the widget.
///
/// # Notes
///
/// This callback is called from glib's main event loop.
void pop_upgrade_widget_callback_error (
    const PopUpgradeWidget *self,
    PopUpgradeWidgetErrorCallback callback,
    void *user_data
);

/// Triggered when the upgrade is occuring, stopped, and complete.
///
/// # Notes
///
/// This callback is called from glib's main event loop.
void pop_upgrade_widget_callback_event (
    const PopUpgradeWidget *self,
    PopUpgradeWidgetEventCallback callback,
    void *user_data
);

/// Triggered when the "Upgrade Ready" notification is clicked.
///
/// # Notes
///
/// This callback is called from glib's main event loop.
void pop_upgrade_widget_callback_ready (
    const PopUpgradeWidget *self,
    PopUpgradeWidgetReadyCallback callback,
    void *user_data
);

GtkWidget *pop_upgrade_widget_container (const PopUpgradeWidget *self);

void pop_upgrade_widget_scan (PopUpgradeWidget *self);

void pop_upgrade_widget_free (PopUpgradeWidget *self);
