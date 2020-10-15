//! All code responsible for validating sources.

use crate::release::eol::{EolDate, EolStatus};
use apt_sources_lists::{SourceEntry, SourceError, SourcesLists};
use distinst_chroot::Command;
use reqwest::Client;
use std::{fs, io, path::Path};
use ubuntu_version::Codename;

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
    #[error(display = "failed to create PPA directory at {}: {}", PPA_SOURCES, _0)]
    PpaDirCreate(io::Error),
}

impl From<SourceError> for SourcesError {
    fn from(why: SourceError) -> Self { SourcesError::ListRead(why) }
}

const MAIN_SOURCES: &str = "/etc/apt/sources.list";
const PPA_SOURCES: &str = "/etc/apt/sources.list.d";

const POP_PPAS: &[&str] = &["system76/pop"];

pub fn repair(codename: Codename) -> Result<(), SourcesError> {
    let current_release = <&'static str>::from(codename);
    if !Path::new(MAIN_SOURCES).exists() {
        info!("/etc/apt/sources.list did not exist: creating a new one");
        return create_new_sources_list(current_release).map_err(SourcesError::ListCreation);
    }

    if !Path::new(PPA_SOURCES).exists() {
        fs::create_dir_all(PPA_SOURCES).map_err(SourcesError::PpaDirCreate)?;
    }

    info!("ensuring that the proprietary pop repo is added");
    let mut sources_list = SourcesLists::scan().map_err(SourcesError::ListRead)?;

    if is_eol(codename) {
        // When EOL, the Ubuntu archives no longer carry packages for that release.
        // Also, disable the proprietary repository before upgrading an EOL release.
        sources_list.entries_mut(|entry| {
            let url = entry.url();
            if let Some(pos) = find_ubuntu_archive(url) {
                let old_release = modify_to_old_release_archive(url, pos);

                if release_exists(&entry, &old_release) {
                    entry.url = old_release;
                }

                true
            } else if entry.url == "http://apt.pop-os.org/proprietary" {
                entry.enabled = false;
                true
            } else {
                false
            }
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

fn is_eol(codename: Codename) -> bool { EolDate::from(codename).status() == EolStatus::Exceeded }

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

pub fn create_new_sources_list(release: &str) -> io::Result<()> {
    fs::write(
        MAIN_SOURCES,
        format!(
            r#"# Ubuntu Repositories

deb http://us.archive.ubuntu.com/ubuntu/ {0} restricted multiverse universe main
deb-src http://us.archive.ubuntu.com/ubuntu/ {0} restricted multiverse universe main

deb http://us.archive.ubuntu.com/ubuntu/ {0}-updates restricted multiverse universe main
deb-src http://us.archive.ubuntu.com/ubuntu/ {0}-updates restricted multiverse universe main

deb http://us.archive.ubuntu.com/ubuntu/ {0}-security restricted multiverse universe main
deb-src http://us.archive.ubuntu.com/ubuntu/ {0}-security restricted multiverse universe main

deb http://us.archive.ubuntu.com/ubuntu/ {0}-backports restricted multiverse universe main
deb-src http://us.archive.ubuntu.com/ubuntu/ {0}-backports restricted multiverse universe main

# Pop!_OS Repositories

deb http://ppa.launchpad.net/system76/pop/ubuntu {0} main
deb-src http://ppa.launchpad.net/system76/pop/ubuntu {0} main

deb http://apt.pop-os.org/proprietary {0} main
"#,
            release
        ),
    )?;

    // TODO: Ensure that the GPG keys are added for the Ubuntu archives.

    Ok(())
}

fn find_ubuntu_archive(url: &str) -> Option<usize> { twoway::find_str(&url, "archive.ubuntu") }

fn modify_to_old_release_archive(url: &str, pos: usize) -> String {
    let stripped = &url[pos + 7..];
    ["http://old-releases", stripped].concat()
}

fn release_exists(entry: &SourceEntry, new_url: &str) -> bool {
    let release_file = [new_url, "/dists/", &entry.suite, "/Release"].concat();
    Client::new().head(&release_file).send().ok().map_or(false, |resp| resp.status().is_success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_releases() {
        let mut url = String::from("http://us.archive.ubuntu.com/ubuntu/");
        let pos = find_ubuntu_archive(&url).unwrap();
        url = modify_to_old_release_archive(&url, pos);
        assert_eq!(&url, "http://old-releases.ubuntu.com/ubuntu/");
    }
}
