use crate::cli::colors::*;
use apt_cmd::AptUpgradeEvent;
use yansi::Paint;

pub fn write_apt_event(event: AptUpgradeEvent) {
    match event {
        AptUpgradeEvent::Processing { package } => {
            println!("{} for {}", color_primary("Processing triggers"), color_secondary(package));
        }
        AptUpgradeEvent::Progress { percent } => {
            println!("{}: {}%", color_primary("Progress"), color_info(percent));
        }
        AptUpgradeEvent::SettingUp { package } => {
            println!("{} {}", color_primary("Setting up"), color_secondary(package));
        }
        AptUpgradeEvent::Unpacking { package, version, over } => {
            println!(
                "{} {} ({}) over ({})",
                color_primary("Unpacking"),
                color_secondary(package),
                color_info(version),
                color_info(over)
            );
        }
        AptUpgradeEvent::WaitingOnLock => {
            println!(
                "{} {}",
                color_primary("Waiting"),
                color_secondary("on a process holding an apt/dpkg lock file")
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
        color_info(event),
        if status == 0 {
            color_primary(success)
        } else {
            inner = format!("{}: {}", color_error(error), color_error_desc(why));

            Paint::wrapping(inner.as_str())
        }
    );
}
