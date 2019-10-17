#include <gtk/gtk.h>

typedef struct { } PopUpgradeWidget;

typedef void (*PopUpgradeWidgetErrorCallback)(const uint8_t*, size_t, void*);

typedef void (*PopUpgradeWidgetEventCallback)(uint8_t, void*);

PopUpgradeWidget *pop_upgrade_widget_new (void);

void pop_upgrade_widget_callback_error (
    const PopUpgradeWidget *self,
    PopUpgradeWidgetErrorCallback callback,
    void *user_data
);

void pop_upgrade_widget_callback_event (
    const PopUpgradeWidget *self,
    PopUpgradeWidgetEventCallback callback,
    void *user_data
)

GtkWidget *pop_upgrade_widget_container (const PopUpgradeWidget *self);

void pop_upgrade_widget_scan (PopUpgradeWidget *self);

void pop_upgrade_widget_free (PopUpgradeWidget *self);
