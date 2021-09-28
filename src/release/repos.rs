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
        apply_default_source_lists(release).context("failed to create new sources.list")?;
    }

    Ok(())
}

fn delete_system76_ubuntu_ppa_list() -> anyhow::Result<()> {
    if let Ok(ppa_directory) = Path::new(PPA_DIR).read_dir() {
        for entry in ppa_directory.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "list") {
                if let Some(fname) = path.file_name() {
                    const POP_PPA: &[u8] = b"system76-ubuntu-pop";
                    if fname.to_raw_bytes().windows(POP_PPA.len()).any(|w| w == POP_PPA) {
                        fs::remove_file(&path).context("failed to remove the old Pop PPA file")?;
                        return Ok(());
                    }
                }
            }
        }
    }

    Ok(())
}

/// For each `.list` in `sources.list.d`, add `#` to the `deb` lines.
pub fn disable_third_parties(release: &str) -> anyhow::Result<()> {
    delete_system76_ubuntu_ppa_list()?;
    let dir = fs::read_dir(PPA_DIR).context("cannot read PPA directory")?;
    for entry in iter_files(dir) {
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "list") {
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

    apply_default_source_lists(release)?;

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

pub fn repair(release: &str) -> anyhow::Result<()> { apply_default_source_lists(release) }

/// If this is an old release, replace `*.archive.ubuntu` sources with `old-releases.ubuntu`
pub fn replace_with_old_releases() -> io::Result<()> {
    let regex = regex::Regex::new("http.*archive.ubuntu.com").expect("bad regex for old-releases");

    let replace = move |input: &str| {
        use std::borrow::Cow;
        match regex.replace_all(input, "http://old-releases.ubuntu.com") {
            Cow::Borrowed(_) => None,
            Cow::Owned(out) => Some(out),
        }
    };

    for source in [SOURCES_LIST, SYSTEM_SOURCES].iter().cloned() {
        if let Ok(contents) = fs::read_to_string(source) {
            if let Some(changed) = replace(&contents) {
                let _ = fs::write(source, changed.as_bytes());
            }
        }
    }

    Ok(())
}

/// Restore a previous backup of the sources lists
pub fn restore(release: &str) -> anyhow::Result<()> {
    info!("restoring release files for {}", release);

    let mut files = Vec::new();

    let sources_list = PathBuf::from([SOURCES_LIST, ".save"].concat());

    if sources_list.exists() {
        files.push(sources_list);
    }

    if let Ok(ppa_directory) = Path::new(PPA_DIR).read_dir() {
        let entries = ppa_directory.filter_map(Result::ok).map(|e| e.path());

        // Inspect what operations we'll need to perform.
        for path in entries {
            let extension = ward::ward!(path.extension(), else { continue });

            if extension == "save" {
                files.push(path);
            }
        }
    }

    for path in files {
        let src_bytes = path.as_os_str().as_bytes();
        let dst_bytes = &src_bytes[..src_bytes.len() - 5];
        let dst_str = OsStr::from_bytes(dst_bytes);
        let dst = Path::new(&dst_str);

        info!("restoring source list at {}", dst.display());

        if dst.exists() {
            if let Err(why) = fs::remove_file(dst) {
                error!("failed to remove source list {}: {}", dst.display(), why);
            }
        }

        if let Err(why) = fs::rename(&path, dst) {
            error!("failed to rename ({}) to ({}): {}", path.display(), dst.display(), why);
        }
    }

    // Ensure default source lists are in place for this release.
    apply_default_source_lists(release)
}

pub fn apply_default_source_lists(release: &str) -> anyhow::Result<()> {
    match release {
        "bionic" | "focal" => {
            info!("creating source repository files for bionic/focal");
            fs::write(SOURCES_LIST, sources_list_before_deb822(release))?;
        }

        "groovy" | "hirsute" => {
            info!("creating source repository files for groovy/hirsute");
            fs::write(SYSTEM_SOURCES, groovy_era_sources(release))?;
            fs::write(GROOVY_PROPRIETARY, groovy_era_proprietary(release))?;
            fs::write(THE_PPA_BEFORE_TIME, the_ppa_before_time(release))?;
            fs::write(SOURCES_LIST, sources_list_placeholder())?;
            delete_system76_ubuntu_ppa_list()?;
        }

        _ => {
            info!("creating source repository files for impish+");

            // Remove any deprecated files on upgrade.
            for file in DEPRECATED_AFTER_FOCAL {
                info!("removing deprecated source at {}", file);
                let _ = fs::remove_file(file)?;
            }

            delete_system76_ubuntu_ppa_list()?;

            info!("creating new sources list");
            fs::write(SOURCES_LIST, sources_list_placeholder())?;
            info!("creating new system sources file");
            fs::write(SYSTEM_SOURCES, impish_era_sources(release))?;
        }
    }

    Ok(())
}

fn impish_era_sources(release: &str) -> String {
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

fn groovy_era_sources(release: &str) -> String {
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

fn sources_list_placeholder() -> String {
    format!(
        r#"## This file is deprecated in Pop!_OS.
## See `man deb822` and /etc/apt/sources.list.d/system.sources.
"#
    )
}

fn groovy_era_proprietary(release: &str) -> String {
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

fn the_ppa_before_time(release: &str) -> String {
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

fn sources_list_before_deb822(release: &str) -> String {
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
