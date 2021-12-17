use crate::release::repos::{iter_files, PPA_DIR};
use anyhow::Context;
use apt_cmd::{AptGet, Dpkg};
use std::fs;
use ubuntu_version::Codename;

pub async fn repair(release: &str) -> anyhow::Result<()> {
    if let Ok(ppas) = std::fs::read_dir(PPA_DIR) {
        for file in iter_files(ppas) {
            let path = file.path();
            if let Ok(contents) = fs::read_to_string(&path) {
                let modified = contents
                    .replace(<&str>::from(Codename::Focal), release)
                    .replace(<&str>::from(Codename::Groovy), release)
                    .replace(<&str>::from(Codename::Hirsute), release)
                    .replace(<&str>::from(Codename::Impish), release);

                if modified != contents {
                    let _ = fs::write(&path, modified.as_bytes());
                }
            }
        }
    }

    AptGet::new()
        .args(&["install", "-f", "-y", "--allow-downgrades"])
        .status()
        .await
        .context("failed to repair broken packages with `apt-get install -f`")?;

    Dpkg::new()
        .configure_all()
        .status()
        .await
        .context("failed to configure packages with `dpkg --configure -a`")
}
