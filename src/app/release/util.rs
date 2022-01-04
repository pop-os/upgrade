use apt_cmd::AptUpgradeEvent;
use yansi::Paint;

use crate::app::color;

pub fn write_apt_event(event: AptUpgradeEvent) {
    match event {
        AptUpgradeEvent::Processing { package } => {
            println!("{} for {}", color::primary("Processing triggers"), color::secondary(package));
        }
        AptUpgradeEvent::Progress { percent } => {
            println!("{}: {}%", color::primary("Progress"), color::info(percent));
        }
        AptUpgradeEvent::SettingUp { package } => {
            println!("{} {}", color::primary("Setting up"), color::secondary(package));
        }
        AptUpgradeEvent::Unpacking { package, version, over } => {
            println!(
                "{} {} ({}) over ({})",
                color::primary("Unpacking"),
                color::secondary(package),
                color::info(version),
                color::info(over)
            );
        }
        AptUpgradeEvent::WaitingOnLock => {
            println!(
                "{} {}",
                color::primary("Waiting"),
                color::secondary("on a process holding an apt/dpkg lock file")
            );
        }
    }
}

pub fn log_result(
    status: u8,
    event: &'static str,
    success: &'static str,
    error: &'static str,
    why: &str,
) {
    let inner: String;

    println!(
        "{}: {}",
        color::info(event),
        if status == 0 {
            color::primary(success)
        } else {
            inner = format!("{}: {}", color::error(error), color::error_desc(why));

            Paint::wrapping(inner.as_str())
        }
    );
}
