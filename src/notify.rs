use notify_rust::{Notification, Timeout};

pub fn notify<F: FnOnce()>(next: &str, func: F) {
    Notification::new()
        .icon("distributor-logo-upgrade-symbolic")
        .summary("Upgrade Available")
        .body(&["Pop!_OS ", next, " is available to download"].concat())
        .action("default", "default")
        .timeout(Timeout::Never)
        .show()
        .expect("failed to show desktop notification")
        .wait_for_action(|action| match action {
            "default" => func(),
            "__closed" => (),
            _ => (),
        });
}
