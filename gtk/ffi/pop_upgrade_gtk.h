#include <glib.h>

typedef struct { } PopUpgradeWidget;

PopUpgradeWidget *pop_upgrade_widget_new (void);

GtkWidget *pop_upgrade_widget_container (const PopUpgradeWidget *self);

int pop_upgrade_widget_scan (PopUpgradeWidget *self);

void pop_upgrade_widget_free (PopUpgradeWidget *self);