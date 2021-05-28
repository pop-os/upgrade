use pop_upgrade::{
    client::{Client, Error as ClientError},
    daemon::DismissEvent,
};

pub fn run(client: &Client) -> Result<(), ClientError> {
    let devel = pop_upgrade::development_releases_enabled();
    let info = client.release_check(devel)?;
    if info.is_lts {
        client.dismiss_notification(DismissEvent::ByUser)?;
    } else {
        println!("Only LTS releases may dismiss notifications");
    }

    Ok(())
}
