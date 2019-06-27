#include <glib.h>
#include <gtk/gtk.h>
#include <pop_upgrade_gtk.h>

gboolean application_quit (GtkWidget *self, GdkEvent *event, PopUpgradeWidget *data) {
    pop_upgrade_widget_free (data);
    gtk_main_quit ();
    return FALSE;
}

static void activate (GApplication *app, gpointer user_data) {
    PopUpgradeWidget *upgrade = pop_upgrade_widget_new ();
    pop_upgrade_widget_scan (upgrade);

    GtkWidget *upgrade_widget = pop_upgrade_widget_container (upgrade);
    gtk_container_set_border_width (GTK_CONTAINER (upgrade_widget), 12);
    gtk_widget_set_margin_top (upgrade_widget, 24);
    gtk_widget_set_halign (upgrade_widget, GTK_ALIGN_CENTER);

    GtkHeaderBar *header = GTK_HEADER_BAR (gtk_header_bar_new ());
    gtk_header_bar_set_title (header, "Pop! Upgrade (C Example)");
    gtk_header_bar_set_show_close_button (header, TRUE);
    gtk_widget_show (GTK_WIDGET (header));
    
    GtkWindow *window = GTK_WINDOW (gtk_application_window_new (GTK_APPLICATION (app)));

    gtk_window_set_icon_name (window, "firmware-manager");
    gtk_window_set_titlebar (window, GTK_WIDGET (header));
    gtk_window_set_keep_above (window, TRUE);
    gtk_window_set_position (window, GTK_WIN_POS_CENTER);
    gtk_container_add (GTK_CONTAINER (window), upgrade_widget);
    gtk_widget_show (GTK_WIDGET (window));

    g_signal_connect (window, "delete-event", G_CALLBACK (application_quit), upgrade);
}

int main (int argc, char **argv) {
    GtkApplication *app = gtk_application_new (
        "com.system76.PopUpgradeExample",
        G_APPLICATION_FLAGS_NONE
    );

    g_signal_connect (app, "activate", G_CALLBACK (activate), NULL);
    
    int status = g_application_run (G_APPLICATION (app), argc, argv);

    g_object_unref (app);
    return status;
}