use notify_rust::{Notification, Timeout};

pub fn notify<F: FnOnce()>(summary: &str, body: &str, func: F) {
    Notification::new()
        .icon("distributor-logo")
        .summary(summary)
        .body(body)
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
