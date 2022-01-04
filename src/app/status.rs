use num_traits::FromPrimitive;
use pop_upgrade::{
    client::Client, daemon::DaemonStatus, recovery::RecoveryEvent, release::UpgradeEvent,
};

pub fn run(client: &Client) -> anyhow::Result<()> {
    let info = client.status()?;

    let (status, sub_status) = match DaemonStatus::from_u8(info.status) {
        Some(status) => {
            let x = <&'static str>::from(status);
            let y = match status {
                DaemonStatus::ReleaseUpgrade => match UpgradeEvent::from_u8(info.sub_status) {
                    Some(sub) => <&'static str>::from(sub),
                    None => "unknown sub_status",
                },
                DaemonStatus::RecoveryUpgrade => match RecoveryEvent::from_u8(info.sub_status) {
                    Some(sub) => <&'static str>::from(sub),
                    None => "unknown sub_status",
                },
                _ => "",
            };

            (x, y)
        }
        None => ("unknown status", ""),
    };

    if sub_status.is_empty() {
        println!("{}", status);
    } else {
        println!("{}: {}", status, sub_status);
    }

    Ok(())
}
