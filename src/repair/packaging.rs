use anyhow::Context;
use apt_cli_wrappers::*;

pub fn repair() -> anyhow::Result<()> {
    apt_install_fix_broken(|_| {})
        .context("failed to repair broken packages with `apt-get install -f`")?;
    dpkg_configure_all(|_| {}).context("failed to configure packages with `dpkg --configure -a`")
}
