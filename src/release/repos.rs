use super::eol::{EolDate, EolStatus};
use anyhow::Context;
use fomat_macros::fomat;
use os_str_bytes::OsStrBytes;
use std::{
    ffi::OsStr,
    fs::{self, DirEntry, ReadDir},
    io,
    path::Path,
};
use ubuntu_version::Codename;

const BACKUP_MAIN_FILE: &str = "/etc/apt/sources.list.save";
const MAIN_FILE: &str = "/etc/apt/sources.list";
const PPA_DIR: &str = "/etc/apt/sources.list.d";
const NEW_MAIN_FILE: &str = "/etc/apt/sources.list.d/system.sources";
const APPS_FILE: &str = "/etc/apt/sources.list.d/pop-os-apps.sources";
const POP_PPA_FILE: &str = "/etc/apt/sources.list.d/pop-os-ppa.list";
const PROPRIETARY_URL: &str = "http://apt.pop-os.org/proprietary";

enum ReleaseSupport {
    BeforeGroovy,
    PostGroovy,
}

impl ReleaseSupport {
    fn get(release: &str) -> anyhow::Result<ReleaseSupport> {
        // The release where DEB822-format sources were adopted.
        const DEB822: &str = "groovy";

        let new = release.parse::<Codename>()?.release_timestamp();

        let groovy = DEB822.parse::<Codename>()?.release_timestamp();

        Ok(if new >= groovy { ReleaseSupport::PostGroovy } else { ReleaseSupport::BeforeGroovy })
    }
}

/// Backup the sources lists
pub fn backup(release: &str) -> anyhow::Result<()> {
    if Path::new(PPA_DIR).exists() {
        // Remove previous backups
        let dir = fs::read_dir(PPA_DIR).context("cannot read PPA directory")?;
        iter_files(dir, |entry| {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "save") {
                log::info!("removing old backup at {}", path.display());
                fs::remove_file(&path)
                    .with_context(|| fomat!("failed to remove backup at "(path.display())))?;
            }

            Ok(())
        })?;

        // Create new backups
        let dir = fs::read_dir(PPA_DIR).context("cannot read PPA directory")?;
        iter_files(dir, |entry| {
            let src_path = entry.path();
            if src_path.extension().map_or(false, |e| e == "list" || e == "sources") {
                let dst_path_buf = [&*(src_path.to_bytes()), b".save"].concat();
                let dst_path_str = OsStr::from_bytes(&dst_path_buf).unwrap();
                let dst_path = Path::new(&dst_path_str);

                log::info!("creating backup of {} to {}", src_path.display(), dst_path.display());
                fs::copy(&src_path, dst_path).with_context(
                    || fomat!("failed to copy " (src_path.display()) " to " (dst_path.display())),
                )?;
            }

            Ok(())
        })?;
    }

    if Path::new(MAIN_FILE).exists() {
        if Path::new(BACKUP_MAIN_FILE).exists() {
            log::info!("removing old backup at {}", BACKUP_MAIN_FILE);
            fs::remove_file(BACKUP_MAIN_FILE).context("failed to remove backup of sources.list")?;
        }
        log::info!("creating backup of {} to {}", MAIN_FILE, BACKUP_MAIN_FILE);
        fs::copy(MAIN_FILE, BACKUP_MAIN_FILE)
            .context("failed to copy sources list to backup path")
            .map(|_| ())
    } else {
        log::info!("sources list was not found — creating a new one");
        create_new_sources_list(release).context("failed to create new sources.list")
    }
}

/// For each `.list` in `sources.list.d`, add `#` to the `deb` lines.
pub fn disable_third_parties(release: &str) -> anyhow::Result<()> {
    let dir = fs::read_dir(PPA_DIR).context("cannot read PPA directory")?;
    iter_files(dir, |entry| {
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "list") {
            if let Some(fname) = path.file_name() {
                const POP_PPA: &[u8] = b"system76-ubuntu-pop";
                if fname.to_bytes().windows(POP_PPA.len()).any(|w| w == POP_PPA) {
                    fs::remove_file(&path).context("failed to remove the old Pop PPA file")?;
                    return Ok(());
                }
            }

            log::info!("disabling sources in {}", path.display());

            let contents = fs::read_to_string(&path)
                .with_context(|| fomat!("failed to read "(&path.display())))?;

            let mut replaced = String::new();
            for line in contents.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("deb") {
                    replaced.push_str("# ")
                }

                replaced.push_str(trimmed);
                replaced.push('\n');
            }

            fs::write(&path, replaced.as_bytes())
                .with_context(|| fomat!("failed to open " (&path.display()) " for writing"))?;
        }

        Ok(())
    })?;

    create_new_sources_list(release)?;

    Ok(())
}

/// Check if an Ubuntu release is EOL'd.
pub fn is_eol(codename: Codename) -> bool {
    EolDate::from(codename).status() == EolStatus::Exceeded
}

// Check if the release exists on Ubuntu's old-releases archive.
pub fn is_old_release(codename: Codename) -> bool {
    let url = &[
        "http://old-releases.ubuntu.com/ubuntu/dists/",
        <&'static str>::from(codename),
        "/Release",
    ]
    .concat();

    isahc::head(url).ok().map_or(false, |resp| resp.status().is_success())
}

pub fn repair(release: &str) -> anyhow::Result<()> {
    if !Path::new(MAIN_FILE).exists() {
        create_new_sources_list(release)?;
    }

    Ok(())
}

/// If this is an old release, replace `*.archive.ubuntu` sources with `old-releases.ubuntu`
pub fn replace_with_old_releases() -> io::Result<()> {
    replace_with_old_releases_(
        || fs::read_to_string(MAIN_FILE),
        |c| fs::write(MAIN_FILE, c.as_bytes()),
    )
}

/// Restore a previous backup of the sources lists
pub fn restore(release: &str) -> anyhow::Result<()> {
    log::info!("restoring release files for {}", release);

    let dir = fs::read_dir(PPA_DIR).context("cannot read PPA directory")?;
    iter_files(dir, |entry| {
        let src_path = entry.path();
        let src_bytes = src_path.to_bytes();
        if src_bytes.ends_with(b".save") {
            let dst_bytes = &src_bytes[..src_bytes.len() - 5];
            let dst_str = OsStr::from_bytes(dst_bytes).unwrap();
            let dst = Path::new(&dst_str);

            log::info!("restoring source list at {}", dst.display());

            if dst.exists() {
                fs::remove_file(dst).with_context(|| fomat!("failed to remove "(dst.display())))?;
            }

            fs::rename(&src_path, dst).with_context(
                || fomat!("failed to rename " (src_path.display()) " to " (dst.display())),
            )?;
        }

        Ok(())
    })?;

    if Path::new(BACKUP_MAIN_FILE).exists() {
        log::info!("restoring system sources list");

        if Path::new(MAIN_FILE).exists() {
            fs::remove_file(MAIN_FILE).context("failed to remove modified system sources.list")?;
        }

        fs::rename(BACKUP_MAIN_FILE, MAIN_FILE).context("failed to restore system sources.list")?;

        if let ReleaseSupport::BeforeGroovy = ReleaseSupport::get(release)? {
            // Also remove these on a groovy upgrade that fails
            if Path::new(NEW_MAIN_FILE).exists() {
                fs::remove_file(NEW_MAIN_FILE).context("failed to remove deb822 system sources")?;
            }

            if Path::new(APPS_FILE).exists() {
                fs::remove_file(APPS_FILE)
                    .context("failed to remove deb822 proprietary sources")?;
            }

            if Path::new(POP_PPA_FILE).exists() {
                fs::remove_file(POP_PPA_FILE).context("failed to remove groovy Pop PPA")?;
            }
        }
    }

    Ok(())
}

fn replace_with_old_releases_(
    read_release: impl FnOnce() -> io::Result<String>,
    write_release: impl FnOnce(String) -> io::Result<()>,
) -> io::Result<()> {
    let mut replaced = String::new();
    let contents = read_release()?;
    for line in contents.lines() {
        let trimmed = line.trim();

        let prefix = if trimmed.starts_with("deb-src") {
            Some("deb-src ")
        } else if trimmed.starts_with("deb") {
            Some("deb ")
        } else {
            None
        };

        if let Some(prefix) = prefix {
            if let Some(pos) = twoway::find_str(trimmed, "archive.ubuntu") {
                replaced.push_str(&[prefix, "http://old-releases", &trimmed[pos + 7..]].concat());
                replaced.push('\n');
                continue;
            }

            // Disable proprietary PPA for old releases
            if trimmed.contains(PROPRIETARY_URL) {
                replaced.push_str("# ");
            }
        }

        replaced.push_str(trimmed);
        replaced.push('\n');
    }

    write_release(replaced)?;

    Ok(())
}

pub fn create_new_sources_list(release: &str) -> anyhow::Result<()> {
    if let ReleaseSupport::PostGroovy = ReleaseSupport::get(release)? {
        // new sources
        fs::write(NEW_MAIN_FILE, new_system_sources(release))?;
        fs::write(APPS_FILE, pop_apps_source(release))?;
        fs::write(POP_PPA_FILE, pop_ppa_source(release))?;
        fs::write(MAIN_FILE, new_sources_file())?;
    } else {
        // old sources
        fs::write(MAIN_FILE, default_sources(release))?;
    }

    // TODO: Ensure that the GPG keys are added for the Ubuntu archives.

    Ok(())
}

pub fn new_system_sources(release: &str) -> String {
    format!(
        r#"X-Repolib-Name: Pop_OS System Sources
Enabled: yes
Types: deb deb-src
URIs: http://us.archive.ubuntu.com/ubuntu/
Suites: {0} {0}-security {0}-updates {0}-backports
Components: main restricted universe multiverse
X-Repolib-Default-Mirror: http://us.archive.ubuntu.com/ubuntu/
"#,
        release
    )
}

pub fn new_sources_file() -> String {
    format!(
        r#"## This file is deprecated in Pop!_OS.
## See `man deb822` and /etc/apt/sources.list.d/system.sources.
"#
    )
}

pub fn pop_apps_source(release: &str) -> String {
    format!(
        r#"X-Repolib-Name: Pop_OS Apps
Enabled: yes
Types: deb
URIs: http://apt.pop-os.org/proprietary
Suites: {0}
Components: main
"#,
        release
    )
}

pub fn pop_ppa_source(release: &str) -> String {
    format!(
        r#"## This file was generated by pop-upgrade
#
## X-Repolib-Name: Pop_OS PPA
deb http://ppa.launchpad.net/system76/pop/ubuntu {0} main
deb-src http://ppa.launchpad.net/system76/pop/ubuntu {0} main
"#,
        release
    )
}

pub fn default_sources(release: &str) -> String {
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
    )
}

fn iter_files(
    dir: ReadDir,
    callback: impl Fn(DirEntry) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    for e in dir {
        let entry = match e {
            Ok(e) => e,
            Err(_) => continue,
        };

        if !entry.metadata().ok().map_or(false, |m| m.is_file()) {
            continue;
        }

        callback(entry)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_release() {
        let codename = Codename::Cosmic;
        let string = <&'static str>::from(codename);

        let contents = default_sources(string);
        let expected = contents
            .replace("us.archive", "old-releases")
            .replace("deb http://apt.pop-os.org", "# deb http://apt.pop-os.org");

        replace_with_old_releases_(
            move || Ok(contents.replace("us.archive", "pl.archive")),
            |contents| {
                assert_eq!(contents, expected);
                Ok(())
            },
        )
        .unwrap();
    }
}
