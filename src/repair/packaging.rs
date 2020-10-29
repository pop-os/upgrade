use anyhow::Context;
use apt_cmd::{AptGet, Dpkg};

pub async fn repair() -> anyhow::Result<()> {
    AptGet::new()
        .args(&["install", "-f"])
        .status()
        .await
        .context("failed to repair broken packages with `apt-get install -f`")?;

    Dpkg::new()
        .configure_all()
        .status()
        .await
        .context("failed to configure packages with `dpkg --configure -a`")
}
