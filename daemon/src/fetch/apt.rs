use anyhow::Context;
use apt_cmd::{lock::apt_lock_wait, request::Request as AptRequest, AptGet};
use async_shutdown::ShutdownManager as Shutdown;
use std::collections::HashSet;

pub enum ExtraPackages {
    Static(&'static [&'static str]),
    Dynamic(Vec<String>),
}

pub async fn fetch_uris(
    shutdown: Shutdown<()>,
    packages: Option<ExtraPackages>,
    dependencies: bool,
) -> anyhow::Result<HashSet<AptRequest>> {
    let task = tokio::spawn(async move {
        apt_lock_wait().await;

        let mut uris = AptGet::new()
            .noninteractive()
            .fetch_uris(&["full-upgrade"])
            .await
            .context("failed to exec `apt-get full-upgrade --print-uris`")?
            .context("failed to fetch package URIs from apt-get full-upgrade")?;

        if let Some(packages) = packages {
            let mut args = if dependencies { vec!["install"] } else { vec!["download"] };
            match packages {
                ExtraPackages::Static(packages) => {
                    args.extend_from_slice(packages);
                }
                ExtraPackages::Dynamic(ref packages) => {
                    if packages.is_empty() {
                        return Ok(uris);
                    }

                    args.extend(packages.iter().map(String::as_str));
                }
            }

            apt_lock_wait().await;

            let install_uris = AptGet::new()
                .noninteractive()
                .fetch_uris(&args)
                .await
                .context("failed to exec `apt-get install --print-uris` or `apt-get download --print-uris`")?
                .context("failed to fetch package URIs from `apt-get install` or `apt-get download`")?;

            for uri in install_uris {
                uris.insert(uri);
            }
        }

        Ok(uris)
    });

    let task = async move { task.await.unwrap() };

    let cancel = async move {
        shutdown.wait_shutdown_triggered().await;
        Err(anyhow::anyhow!("process interrupted by cancelation"))
    };

    futures::pin_mut!(cancel);
    futures::pin_mut!(task);

    futures::future::select(cancel, task).await.factor_first().0
}
