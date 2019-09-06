use notify_rust::{Notification, Timeout};

pub fn notify<F: FnMut()>(icon: &str, summary: &str, body: &str, mut func: F) {
    Notification::new()
        .icon(icon)
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
