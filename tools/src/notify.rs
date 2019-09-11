use std::{env, fs::File, io::Write};

markup::define! {
    Service<'a>(description: &'a str, appid: &'a str, exec: &'a str) {
        "[Unit]\n"
        "Description=" { markup::raw(description) } "\n"
        "Wants=" { markup::raw(appid) } ".timer\n"
        "\n"
        "[Service]\n"
        "ExecStart=" { markup::raw(exec) } "\n"
        "\n"
        "[Install]\n"
        "WantedBy=default.target\n"
    }
}

markup::define! {
    Timer<'a>(description: &'a str, appid: &'a str, minutes: u16) {
        "[Unit]\n"
        "Description=" { markup::raw(description) } "\n"
        "Requires=" { markup::raw(appid) } ".service\n"
        "\n"
        "[Timer]\n"
        "Unit=" { markup::raw(appid) } ".service\n"
        "OnUnitInactiveSec=" { markup::raw(minutes) } "m\n"
        "AccuracySec=1s\n"
        "\n"
        "[Install]\n"
        "WantedBy=timers.target\n"
    }
}

fn main() {
    let service = "pop-upgrade-notify";
    let prefix = env::var("prefix").unwrap();

    let timer_path = ["target/", service, ".timer"].concat();
    let service_path = ["target/", service, ".service"].concat();
    let exec = [&prefix, "/bin/pop-upgrade release check"].concat();

    let timer = Timer {
        description: "Checks for new OS releases every day",
        appid:       service,
        minutes:     1440,
    };

    let service = Service {
        description: "Check for a new OS release, and display a notification if found",
        appid:       service,
        exec:        &exec,
    };

    File::create(timer_path)
        .expect("failed to create timer service")
        .write_all(timer.to_string().as_bytes())
        .expect("failed to write timer service");

    File::create(service_path)
        .expect("failed to create service service")
        .write_all(service.to_string().as_bytes())
        .expect("failed to write service service");
}
