use num_traits::cast::FromPrimitive;
use pop_upgrade::{
    client::{Client, Error as ClientError},
    daemon::DaemonStatus,
    recovery::RecoveryEvent,
    release::UpgradeEvent,
};

pub fn run(client: &Client) -> Result<(), ClientError> {
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
