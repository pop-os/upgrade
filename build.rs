use std::{env, fs::File, io::Write};

const NOTIFY_SERVICE: &str = "pop-upgrade-notify";

fn systemd_service(description: &str, appid: &str, exec: &str) -> String {
    fomat_macros::fomat!(
        "[Unit]\n"
        "Description=" (description) "\n"
        "Wants=" (appid) ".timer\n"
        "\n"
        "[Service]\n"
        "ExecStart=" (exec) "\n"
        "\n"
        "[Install]\n"
        "WantedBy=default.target\n"
    )
}

fn systemd_timer(description: &str, appid: &str, minutes: u16) -> String {
    fomat_macros::fomat!(
        "[Unit]\n"
        "Description=" (description) "\n"
        "Requires=" (appid) ".service\n"
        "\n"
        "[Timer]\n"
        "Unit=" (appid) ".service\n"
        "OnUnitInactiveSec=" (minutes) "m\n"
        "AccuracySec=1s\n"
        "\n"
        "[Install]\n"
        "WantedBy=timers.target\n"
    )
}

fn main() {
    let prefix = env::var("prefix").unwrap();

    let timer_path = ["target/", NOTIFY_SERVICE, ".timer"].concat();
    let service_path = ["target/", NOTIFY_SERVICE, ".service"].concat();
    let exec = [&prefix, "/bin/pop-upgrade release check"].concat();

    let timer = systemd_timer("Checks for new OS releases every day", NOTIFY_SERVICE, 1440);
    let service = systemd_service("Check for a new OS release, and display a notification if found", NOTIFY_SERVICE, &exec);

    File::create(timer_path)
        .expect("failed to create timer service")
        .write_all(timer.as_bytes())
        .expect("failed to write timer service");

    File::create(service_path)
        .expect("failed to create service service")
        .write_all(service.as_bytes())
        .expect("failed to write service service");

    let desktop = include_str!("data/com.system76.PopUpgrade.Notify.desktop")
        .replace("{{exec}}", &fomat_macros::fomat!((prefix) "/bin/pop-upgrade release check"));

    File::create("target/com.system76.PopUpgrade.Notify.desktop")
        .expect("failed to create desktop file for notification service")
        .write_all(desktop.as_bytes())
        .expect("failed to write desktop file for notification service");

}
