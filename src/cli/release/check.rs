use crate::notify::notify;
use chrono::{offset::TimeZone, Utc};
use pop_upgrade::{
    client::{Client, Error as ClientError},
    misc,
    release::eol::{EolDate, EolStatus},
};

pub fn run(client: &Client) -> Result<(), ClientError> {
    let mut buffer = String::new();
    let info = client.release_check(false)?;

    if atty::is(atty::Stream::Stdout) {
        println!(
            "      Current Release: {}\n         Next Release: {}\nNew Release Available: {}",
            info.current,
            info.next,
            misc::format_build_number(info.build, &mut buffer)
        );
    } else if info.build >= 0 {
        if info.is_lts
            && (super::dismissed(&info.next) || super::dismiss_by_timestamp(client, &info.next)?)
        {
            return Ok(());
        }

        let (summary, body) = notification_message(&info.current, &info.next);

        let upgrade_panel = if &*info.current == "18.04" { "info-overview" } else { "upgrade" };

        notify(&summary, &body, || {
            let _ = exec::Command::new("gnome-control-center").arg(upgrade_panel).exec();
        });
    }

    Ok(())
}

fn notification_message(current: &str, next: &str) -> (String, String) {
    match EolDate::fetch() {
        Ok(eol) => match eol.status() {
            EolStatus::Exceeded => {
                return (
                    fomat!("Support for Pop!_OS " (current) " has ended"),
                    fomat!(
                        "Security and application updates are no longer provided for Pop!_OS "
                        (current) ". Upgrade to Pop!_OS " (next) " to keep your computer secure."
                    ),
                );
            }
            EolStatus::Imminent => {
                let (y, m, d) = eol.ymd;
                return (
                    fomat!(
                        "Support for Pop!_OS " (current) " ends "
                        (Utc.ymd(y as i32, m, d).format("%B %-d, %Y"))
                    ),
                    fomat!(
                        "This computer will soon stop receiving updates"
                        ". Upgrade to Pop!_OS " (next) " to keep your computer secure."
                    ),
                );
            }
            EolStatus::Ok => (),
        },
        Err(why) => error!("failed to fetch EOL date: {}", why),
    }

    ("Upgrade Available".into(), fomat!("Pop!_OS " (next) " is available to download"))
}
