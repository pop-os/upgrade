use apt_cli_wrappers::*;

#[derive(Debug, Error)]
pub enum Error {
    #[error(display = "failed to repair broken packages with `apt-get install -f`")]
    FixBroken,
    #[error(display = "failed to configure packages with `dpkg --configure -a`")]
    DpkgConfigure,
}

pub fn repair() -> Result<(), Error> {
    apt_install_fix_broken(|_| {}).map_err(|_| Error::FixBroken)?;
    dpkg_configure_all(|_| {}).map_err(|_| Error::DpkgConfigure)?;

    Ok(())
}
