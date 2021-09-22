use super::eol::{EolDate, EolStatus};
use anyhow::Context;
use os_str_bytes::OsStrBytes;
use std::{
    ffi::OsStr,
    fs::{self, DirEntry, ReadDir},
    io,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};
use ubuntu_version::Codename;

const SOURCES_LIST: &str = "/etc/apt/sources.list";
const PPA_DIR: &str = "/etc/apt/sources.list.d";
const SYSTEM_SOURCES: &str = "/etc/apt/sources.list.d/system.sources";
const PROPRIETARY_URL: &str = "http://apt.pop-os.org/proprietary";
const GROOVY_PROPRIETARY: &str = "/etc/apt/sources.list.d/pop-os-apps.sources";
const THE_PPA_BEFORE_TIME: &str = "/etc/apt/sources.list.d/pop-os-ppa.list";

const DEPRECATED_AFTER_FOCAL: &[&str] =
    &["/etc/apt/sources.list.d/pop-os-apps.sources", "/etc/apt/sources.list.d/pop-os-ppa.list"];

/// Backup the sources lists
pub fn backup(release: &str) -> anyhow::Result<()> {
    // Files that have been marked for deletion.
    let mut delete = Vec::new();

    // Files that will be backed up with a `.save` extension.
    let mut backup = Vec::new();

    // Track if the main sources.list file is missing.
    let mut sources_missing = false;

    // Backup the sources lists
    if Path::new(SOURCES_LIST).exists() {
        let backup_path = PathBuf::from([SOURCES_LIST, ".save"].concat());

        if backup_path.exists() {
            delete.push(backup_path);
        }

        backup.push(PathBuf::from(SOURCES_LIST));
    } else {
        sources_missing = true;
    }

    if let Ok(ppa_directory) = Path::new(PPA_DIR).read_dir() {
        // Inspect what operations we'll need to perform.
        for entry in ppa_directory.filter_map(Result::ok) {
            let path = entry.path();

            if let Some(extension) = path.extension() {
                if extension == "save" {
                    delete.push(path);
                } else if extension == "sources" || extension == "list" {
                    backup.push(path);
                }
            }
        }
    }

    // Delete old backups first.
    for path in &delete {
        info!("removing old backup at {}", path.display());
        fs::remove_file(&path)
            .with_context(|| fomat!("failed to remove backup at "(path.display())))?;
    }

    // Then create new backups.
    for src in &backup {
        let dst_path_buf = [&*(src.to_raw_bytes()), b".save"].concat();
        let dst_path_str = OsStr::from_bytes(&dst_path_buf);
        let dst_path = Path::new(&dst_path_str);

        info!("creating backup of {} to {}", src.display(), dst_path.display());
        fs::copy(&src, dst_path).with_context(
            || fomat!("failed to copy " (src.display()) " to " (dst_path.display())),
        )?;
    }

    if sources_missing {
        info!("sources list was not found — creating a new one");
        create_new_sources_list(release).context("failed to create new sources.list")?;
    }

    Ok(())
}

/// For each `.list` in `sources.list.d`, add `#` to the `deb` lines.
pub fn disable_third_parties(release: &str) -> anyhow::Result<()> {
    let dir = fs::read_dir(PPA_DIR).context("cannot read PPA directory")?;
    for entry in iter_files(dir) {
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "list") {
            if let Some(fname) = path.file_name() {
                const POP_PPA: &[u8] = b"system76-ubuntu-pop";
                if fname.to_raw_bytes().windows(POP_PPA.len()).any(|w| w == POP_PPA) {
                    fs::remove_file(&path).context("failed to remove the old Pop PPA file")?;
                    return Ok(());
                }
            }

            info!("disabling sources in {}", path.display());

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
    }

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
    if !Path::new(SOURCES_LIST).exists() {
        create_new_sources_list(release)?;
    }

    Ok(())
}

/// If this is an old release, replace `*.archive.ubuntu` sources with `old-releases.ubuntu`
pub fn replace_with_old_releases() -> io::Result<()> {
    replace_with_old_releases_(
        || fs::read_to_string(SOURCES_LIST),
        |c| fs::write(SOURCES_LIST, c.as_bytes()),
    )
}

/// Restore a previous backup of the sources lists
pub fn restore(release: &str) -> anyhow::Result<()> {
    info!("restoring release files for {}", release);

    let dir = fs::read_dir(PPA_DIR).context("cannot read PPA directory")?;
    for entry in iter_files(dir) {
        let src_path = entry.path();
        let src_bytes = src_path.to_raw_bytes();
        if src_bytes.ends_with(b".save") {
            let dst_bytes = &src_bytes[..src_bytes.len() - 5];
            let dst_str = OsStr::from_bytes(dst_bytes);
            let dst = Path::new(&dst_str);

            info!("restoring source list at {}", dst.display());

            if dst.exists() {
                fs::remove_file(dst).with_context(|| fomat!("failed to remove "(dst.display())))?;
            }

            fs::rename(&src_path, dst).with_context(
                || fomat!("failed to rename " (src_path.display()) " to " (dst.display())),
            )?;
        }
    }

    let backup = [SOURCES_LIST, ".save"].concat();

    if Path::new(&backup).exists() {
        info!("restoring system sources list");

        if Path::new(SOURCES_LIST).exists() {
            fs::remove_file(SOURCES_LIST)
                .context("failed to remove modified system sources.list")?;
        }

        fs::rename(&backup, SOURCES_LIST).context("failed to restore system sources.list")?;

        // If reverting to focal sources
        if release == "focal" {
            // Also remove these on a groovy upgrade that fails
            if Path::new(SYSTEM_SOURCES).exists() {
                fs::remove_file(SYSTEM_SOURCES)
                    .context("failed to remove deb822 system sources")?;
            }

            if Path::new(GROOVY_PROPRIETARY).exists() {
                fs::remove_file(GROOVY_PROPRIETARY)
                    .context("failed to remove deb822 proprietary sources")?;
            }

            if Path::new(THE_PPA_BEFORE_TIME).exists() {
                fs::remove_file(THE_PPA_BEFORE_TIME).context("failed to remove groovy Pop PPA")?;
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
    match release {
        "bionic" | "focal" => {
            fs::write(SOURCES_LIST, sources_list_before_deb822(release))?;
        }

        "groovy" | "hirsute" => {
            fs::write(SYSTEM_SOURCES, groovy_era_sources(release))?;
            fs::write(GROOVY_PROPRIETARY, groovy_era_proprietary(release))?;
            fs::write(THE_PPA_BEFORE_TIME, the_ppa_before_time(release))?;
            fs::write(SOURCES_LIST, sources_list_placeholder())?;
        }

        _ => {
            // Remove any deprecated files on upgrade.
            for file in DEPRECATED_AFTER_FOCAL {
                let _ = fs::remove_file(file)?;
            }

            fs::write(SOURCES_LIST, sources_list_placeholder())?;
            fs::write(SYSTEM_SOURCES, impish_era_sources(release))?;
        }
    }

    Ok(())
}

pub fn impish_era_sources(release: &str) -> String {
    format!(
        r#"X-Repolib-Name: Pop_OS System Sources
Enabled: yes
Types: deb deb-src
URIs: http://us.archive.ubuntu.com/ubuntu/
Suites: {0} {0}-security {0}-updates {0}-backports
Components: main restricted universe multiverse
X-Repolib-Default-Mirror: http://us.archive.ubuntu.com/ubuntu/
    
X-Repolib-Name: Pop_OS Release Sources
Enabled: yes
Types: deb deb-src
URIs: http://apt.pop-os.org/release
Suites: {0}
Components: main

X-Repolib-Name: Pop_OS Apps
Enabled: yes
Types: deb
URIs: http://apt.pop-os.org/proprietary
Suites: {0}
Components: main
"#,
        release
    )
}

pub fn groovy_era_sources(release: &str) -> String {
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

pub fn sources_list_placeholder() -> String {
    format!(
        r#"## This file is deprecated in Pop!_OS.
## See `man deb822` and /etc/apt/sources.list.d/system.sources.
"#
    )
}

pub fn groovy_era_proprietary(release: &str) -> String {
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

pub fn the_ppa_before_time(release: &str) -> String {
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

pub fn sources_list_before_deb822(release: &str) -> String {
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

fn iter_files(dir: ReadDir) -> impl Iterator<Item = DirEntry> {
    dir.filter_map(Result::ok).filter(|entry| !entry.metadata().ok().map_or(false, |m| m.is_file()))
}
