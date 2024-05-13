use crate::{
    release::repos::{iter_files, PPA_DIR},
    ubuntu_version::Codename,
};
use anyhow::Context;
use apt_cmd::{lock::apt_lock_wait, AptGet, Dpkg};
use futures::StreamExt;
use std::fs;

pub async fn repair(release: &str) -> anyhow::Result<()> {
    apt_lock_wait().await;
    if let Ok(ppas) = std::fs::read_dir(PPA_DIR) {
        for file in iter_files(ppas) {
            let path = file.path();
            if let Ok(contents) = fs::read_to_string(&path) {
                let modified = contents
                    .replace(<&str>::from(Codename::Focal), release)
                    .replace(<&str>::from(Codename::Groovy), release)
                    .replace(<&str>::from(Codename::Hirsute), release)
                    .replace(<&str>::from(Codename::Impish), release)
                    .replace(<&str>::from(Codename::Jammy), release)
                    .replace(<&str>::from(Codename::Noble), release);

                if modified != contents {
                    let _ = fs::write(&path, modified.as_bytes());
                }
            }
        }
    }

    if crate::release::repos::is_old_release(release).await {
        info!("switching to old-releases repositories");
        let _ = crate::release::repos::replace_with_old_releases();
    }

    apt_lock_wait().await;
    let _ = AptGet::new().update().await;

    let mut last_error = Ok(());

    for _ in 0..3i32 {
        apt_lock_wait().await;
        let a = crate::misc::apt_get()
            .fix_broken()
            .status()
            .await
            .context("failed to repair broken packages with `apt-get install -f`");

        apt_lock_wait().await;
        let b = Dpkg::new()
            .force_confdef()
            .force_confold()
            .configure_all()
            .status()
            .await
            .context("failed to configure packages with `dpkg --configure -a`");

        last_error = a.and(b).and(base_requirements().await);

        if last_error.is_ok() {
            break;
        }
    }

    last_error
}

const PROBLEMATIC_PACKAGES: &[&str] = &[
    "zlib1g",
    "libc6",
    "libc6:i386",
    "ppp",
    "libnm0",
    "libc++1",
    "libc++1:i386",
    "libmount1:i386",
];

async fn base_requirements() -> anyhow::Result<()> {
    info!("ensuring prerequisites are installed");

    // Fetch apt-cache policies for each of the problematic packages.
    let (mut child, policies) = apt_cmd::AptCache::new().policy(PROBLEMATIC_PACKAGES).await?;

    // Remember which packages are installed with candidates.
    let mut to_install = Vec::new();

    futures_util::pin_mut!(policies);

    while let Some(policy) = policies.next().await {
        if policy.installed != "(none)" && policy.installed != policy.candidate {
            to_install.push(format!("{}={}", policy.package, policy.candidate));
        }
    }

    let _ = child.wait();

    info!("installing required prerequisites: {:?}", to_install);

    // Ensure that the packages have their candidate versions installed.
    crate::misc::apt_get().install(to_install).await.context("failed to install prerequisites")
}
