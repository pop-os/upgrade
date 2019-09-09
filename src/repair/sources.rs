//! All code responsible for validating sources.

use apt_sources_lists::{SourceEntry, SourceError, SourcesLists};
use distinst_chroot::Command;
use std::{fs, io, path::Path};
use ubuntu_version::{Codename, Version};

#[derive(Debug, Error)]
pub enum SourcesError {
    #[error(display = "/etc/apt/sources.list was missing, and we failed to create it: {}", _0)]
    ListCreation(io::Error),
    #[error(display = "failed to read sources: {}", _0)]
    ListRead(SourceError),
    #[error(display = "failed to overwrite a source list: {}", _0)]
    ListWrite(io::Error),
    #[error(display = "failed to add missing PPA from Launchpad: {}", _0)]
    PpaAdd(io::Error),
}

impl From<SourceError> for SourcesError {
    fn from(why: SourceError) -> Self { SourcesError::ListRead(why) }
}

const MAIN_SOURCES: &str = "/etc/apt/sources.list";

const POP_PPAS: &[&str] = &["system76/pop"];

pub fn repair(codename: Codename) -> Result<(), SourcesError> {
    let eol = is_eol(codename);

    let current_release = <&'static str>::from(codename);
    if !Path::new(MAIN_SOURCES).exists() {
        info!("/etc/apt/sources.list did not exist: creating a new one");
        return create_new_sources_list(current_release).map_err(SourcesError::ListCreation);
    }

    info!("ensuring that the proprietary pop repo is added");
    let mut sources_list = SourcesLists::scan().map_err(SourcesError::ListRead)?;

    if eol {
        // When EOL, the Ubuntu archives no longer carry packages for that release.
        sources_list.entries_mut(|entry| {
            let mut modified = false;

            if entry.url.contains("us.archive") {
                entry.url = entry.url.replace("us.archive", "old-releases");
                modified = true;
            }

            // Disable the proprietary repository before upgrading an EOL release.
            if entry.url == "http://apt.pop-os.org/proprietary" {
                entry.enabled = false;
                modified = true;
            }

            modified
        });
    } else {
        insert_entry(
            &mut sources_list,
            MAIN_SOURCES,
            "http://apt.pop-os.org/proprietary",
            current_release,
            &["main"],
        )?;
    }

    sources_list.write_sync().map_err(SourcesError::ListWrite)?;

    for ppa in POP_PPAS {
        let url = ["http://ppa.launchpad.net/", *ppa, "/ubuntu"].concat();
        if sources_list.iter().any(|file| file.contains_entry(&url).is_some()) {
            info!("PPA {} found: not adding", *ppa);
        } else {
            info!("adding PPA: {}", *ppa);
            ppa_add(*ppa)?;
        }
    }

    Ok(())
}

fn is_eol(codename: Codename) -> bool {
    let this_version = Version::from(codename);
    let until_eol = if this_version.is_lts() { 120 } else { 9 };
    this_version.months_since() > until_eol
}

fn ppa_add(ppa: &str) -> Result<(), SourcesError> {
    Command::new("add-apt-repository")
        .arg(format!("ppa:{}", ppa))
        .arg("-ny")
        .run()
        .map_err(SourcesError::PpaAdd)
}

fn insert_entry<P: AsRef<Path>>(
    sources_list: &mut SourcesLists,
    preferred: P,
    url: &str,
    suite: &str,
    components: &[&str],
) -> Result<(), SourcesError> {
    sources_list.insert_entry(
        preferred,
        SourceEntry {
            enabled:    true,
            source:     false,
            options:    None,
            url:        url.to_owned(),
            suite:      suite.to_owned(),
            components: components.iter().cloned().map(String::from).collect(),
        },
    )?;

    Ok(())
}

fn create_new_sources_list(release: &str) -> io::Result<()> {
    fs::write(MAIN_SOURCES, format!(
        "deb http://us.archive.ubuntu.com/ubuntu/ {0} restricted multiverse universe main\n\
         deb-src http://us.archive.ubuntu.com/ubuntu/ {0} restricted multiverse universe main\n\
         deb http://us.archive.ubuntu.com/ubuntu/ {0}-updates restricted multiverse universe main\n\
         deb-src http://us.archive.ubuntu.com/ubuntu/ {0}-updates restricted multiverse universe main\n\
         deb http://us.archive.ubuntu.com/ubuntu/ {0}-security restricted multiverse universe main\n\
         deb-src http://us.archive.ubuntu.com/ubuntu/ {0}-security restricted multiverse universe main\n\
         deb http://us.archive.ubuntu.com/ubuntu/ {0}-backports restricted multiverse universe main\n\
         deb-src http://us.archive.ubuntu.com/ubuntu/ {0}-backports restricted multiverse universe main\n\
         deb http://apt.pop-os.org/proprietary {0} main\n",
         release
    ))?;

    // TODO: Ensure that the GPG keys are added for the Ubuntu archives.

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
}
