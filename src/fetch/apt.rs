use anyhow::Context;
use apt_cmd::{lock::apt_lock_wait, request::Request as AptRequest, AptGet};
use std::collections::HashSet;

pub async fn fetch_uris(packages: Option<&[&str]>) -> anyhow::Result<HashSet<AptRequest>> {
    apt_lock_wait().await;
    let mut uris = AptGet::new()
        .noninteractive()
        .fetch_uris(&["full-upgrade"])
        .await
        .context("failed to exec `apt-get full-upgrade --print-uris`")?
        .context("failed to fetch package URIs from apt-get full-upgrade")?;

    if let Some(packages) = packages {
        apt_lock_wait().await;
        let install_uris = AptGet::new()
            .noninteractive()
            .fetch_uris(&{
                let mut args = vec!["install"];
                args.extend_from_slice(packages);
                args
            })
            .await
            .context("failed to exec `apt-get install --print-uris`")?
            .context("failed to fetch package URIs from `apt-get install`")?;

        for uri in install_uris {
            uris.insert(uri);
        }
    }

    Ok(uris)
}
