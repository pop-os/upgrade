use super::eol::{EolDate, EolStatus};
use crate::ubuntu_version::Codename;
use anyhow::Context;
use const_format::concatcp;
use os_str_bytes::{OsStrBytes, OsStrBytesExt};
use std::{
    ffi::OsStr,
    fs::{self, DirEntry, ReadDir},
    io,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

const SOURCES_LIST: &str = "/etc/apt/sources.list";
pub const PPA_DIR: &str = concatcp!(SOURCES_LIST, ".d/");
const SYSTEM_SOURCES: &str = concatcp!(PPA_DIR, "system.sources");
const PROPRIETARY_SOURCES: &str = concatcp!(PPA_DIR, "pop-os-apps.sources");
const GROOVY_PPA: &str = concatcp!(PPA_DIR, "pop-os-ppa.list");
const PPA_SOURCES: &str = concatcp!(PPA_DIR, "pop-os-ppa.sources");
const IMPISH_RELEASE: &str = concatcp!(PPA_DIR, "pop-os-release.sources");

const REMOVE_LIST: &[&str] =
    &[SYSTEM_SOURCES, PROPRIETARY_SOURCES, GROOVY_PPA, IMPISH_RELEASE, PPA_SOURCES];

/// Backup the sources lists
pub async fn backup(release: &str) -> anyhow::Result<()> {
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
        let dst_path_buf = [src.as_os_str().as_bytes(), b".save"].concat();
        let dst_path_str = OsStr::from_bytes(&dst_path_buf);
        let dst_path = Path::new(&dst_path_str);

        info!("creating backup of {} to {}", src.display(), dst_path.display());
        fs::copy(&src, dst_path).with_context(
            || fomat!("failed to copy " (src.display()) " to " (dst_path.display())),
        )?;
    }

    if sources_missing {
        info!("sources list was not found — creating a new one");
        apply_default_source_lists(release).await.context("failed to create new sources.list")?;
    }

    Ok(())
}

fn delete_system76_ubuntu_ppa_list() {
    if let Ok(ppa_directory) = Path::new(PPA_DIR).read_dir() {
        for entry in ppa_directory.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "list") {
                if let Some(fname) = path.file_name() {
                    if fname.contains("system76-ubuntu-pop") {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        }
    }
}

/// For each `.list` in `sources.list.d`, add `#` to the `deb` lines.
pub async fn disable_third_parties(release: &str) -> anyhow::Result<()> {
    delete_system76_ubuntu_ppa_list();
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
                    replaced.push_str("# ");
                }

                replaced.push_str(trimmed);
                replaced.push('\n');
            }

            fs::write(&path, replaced.as_bytes())
                .with_context(|| fomat!("failed to open " (&path.display()) " for writing"))?;
        } else if path.extension().map_or(false, |e| e == "sources") {
            if let Some(fname) = path.file_name() {
                if !(fname.starts_with("pop-os") || fname.starts_with("system")) {
                    let _ = fs::remove_file(&path);
                }
            }
        };
    }

    apply_default_source_lists(release).await?;

    Ok(())
}

/// Check if an Ubuntu release is EOL'd.
pub fn is_eol(codename: Codename) -> bool {
    EolDate::from(codename).status() == EolStatus::Exceeded
}

// Check if the release exists on Ubuntu's old-releases archive.
pub async fn is_old_release(codename: &str) -> bool {
    let url = &["http://old-releases.ubuntu.com/ubuntu/dists/", codename, "/Release"].concat();

    if let Ok(client) = crate::misc::http_client() {
        let request = || async { client.head_async(url).await };
        if let Ok(resp) = crate::misc::network_reconnect(request).await {
            return resp.status().is_success();
        }
    }

    false
}

pub async fn repair(release: &str) -> anyhow::Result<()> {
    apply_default_source_lists(release).await
}

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

    for source in &[SOURCES_LIST, SYSTEM_SOURCES] {
        if let Ok(contents) = fs::read_to_string(source) {
            if let Some(changed) = replace(&contents) {
                let _ = fs::write(source, changed.as_bytes());
            }
        }
    }

    Ok(())
}

/// Restore a previous backup of the sources lists
pub async fn restore(release: &str) -> anyhow::Result<()> {
    info!("restoring release files for {}", release);

    // Start by removing all of the non-.save files, if .save files exist.
    if let Ok(dir) = fs::read_dir(PPA_DIR) {
        info!("checking for extra source files that should be removed.");

        if dbg!(iter_files(dir).any(|entry| is_save_file(&entry.path()))) {
            info!("found save files to restore");

            if let Ok(dir) = fs::read_dir(PPA_DIR) {
                info!("removing sources which lack .save backups");
                for entry in iter_files(dir) {
                    let path = entry.path();
                    info!("checking if {:?} should be removed", path);
                    if !is_save_file(&path) {
                        info!("removing {:?}: {:?}", path, fs::remove_file(&path));
                    }
                }
            }
        }
    }

    for file in REMOVE_LIST {
        let _ = fs::remove_file(file);
        let _ = fs::remove_file(&*[file, ".save"].concat());
    }

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
    let a = apply_default_source_lists(release).await;
    let b = update_preferences_script(release);

    if release == "focal" {
        let _ = fs::remove_file("/etc/apt/sources.list.d/system76-ubuntu-pop-focal.list");
    }

    a.or(b)
}

pub async fn apply_default_source_lists(release: &str) -> anyhow::Result<()> {
    match release {
        "bionic" | "focal" => {
            info!("creating source repository files for bionic/focal");
            fs::write(SOURCES_LIST, sources_list_before_deb822(release))?;
        }

        "groovy" | "hirsute" => {
            info!("creating source repository files for groovy/hirsute");
            fs::write(SOURCES_LIST, sources_list_placeholder())?;
            fs::write(SYSTEM_SOURCES, system_sources(release))?;
            fs::write(PROPRIETARY_SOURCES, proprietary_sources(release))?;
            fs::write(GROOVY_PPA, groovy_ppa(release))?;
            delete_system76_ubuntu_ppa_list();
        }

        _ => {
            info!("creating source repository files for impish+");
            let _ = fs::remove_file(GROOVY_PPA);
            let _ = fs::remove_file(PPA_SOURCES);
            fs::write(SOURCES_LIST, sources_list_placeholder())?;
            fs::write(SYSTEM_SOURCES, system_sources(release))?;
            fs::write(PROPRIETARY_SOURCES, proprietary_sources(release))?;
            fs::write(IMPISH_RELEASE, release_sources(release))?;
            delete_system76_ubuntu_ppa_list();
        }
    }

    update_preferences_script(release)?;

    if is_old_release(release).await {
        let _ = replace_with_old_releases();
    }

    Ok(())
}

/// Apt preferences for Bionic through Hirsute
const PREFERENCES_BIONIC: &str = r#"Package: *
Pin: release o=LP-PPA-system76-pop
Pin-Priority: 1001

Package: *
Pin: release o=LP-PPA-system76-proposed
Pin-Priority: 1001
"#;

/// Apt preferences for Impish and beyond
const PREFERENCES_IMPISH: &str = r#"Package: *
Pin: release o=pop-os-release
Pin-Priority: 1001
"#;

/// Overwrites Pop's apt preferences script
fn update_preferences_script(release: &str) -> anyhow::Result<()> {
    let data = match release {
        "bionic" | "focal" | "hirsute" => PREFERENCES_BIONIC,
        _ => PREFERENCES_IMPISH,
    };

    fs::write("/etc/apt/preferences.d/pop-default-settings", data.as_bytes())
        .context("failed to overwrite pop-default-settings apt preferences")
}

fn ubuntu_uri() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "ports.ubuntu.com/ubuntu-ports"
    } else {
        "us.archive.ubuntu.com/ubuntu"
    }
}

fn system_sources(release: &str) -> String {
    let uri =
        if cfg!(target_arch = "aarch64") { ubuntu_uri() } else { "apt.pop-os.org/ubuntu" };
    format!(
        r#"X-Repolib-Name: Pop_OS System Sources
Enabled: yes
Types: deb deb-src
URIs: http://{1}
Suites: {0} {0}-security {0}-updates {0}-backports
Components: main restricted universe multiverse
X-Repolib-Default-Mirror: http://{1}
"#,
        release, uri
    )
}

fn sources_list_placeholder() -> String {
    r#"## This file is deprecated in Pop!_OS.
## See `man deb822` and /etc/apt/sources.list.d/system.sources.
"#
    .to_string()
}

fn proprietary_sources(release: &str) -> String {
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

fn release_sources(release: &str) -> String {
    format!(
        r#"X-Repolib-Name: Pop_OS Release Sources
Enabled: yes
Types: deb deb-src
URIs: http://apt.pop-os.org/release
Suites: {0}
Components: main
"#,
        release
    )
}

fn groovy_ppa(release: &str) -> String {
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

deb http://{1} {0} restricted multiverse universe main
deb-src http://{1} {0} restricted multiverse universe main

deb http://{1} {0}-updates restricted multiverse universe main
deb-src http://{1} {0}-updates restricted multiverse universe main

deb http://{1} {0}-security restricted multiverse universe main
deb-src http://{1} {0}-security restricted multiverse universe main

deb http://{1} {0}-backports restricted multiverse universe main
deb-src http://{1} {0}-backports restricted multiverse universe main

# Pop!_OS Repositories

deb http://ppa.launchpad.net/system76/pop/ubuntu {0} main
deb-src http://ppa.launchpad.net/system76/pop/ubuntu {0} main
{2}"#,
        release,
        ubuntu_uri(),
        if cfg!(target_arch = "aarch64") {
            String::new()
        } else {
            format!("deb http://apt.pop-os.org/proprietary {} main", release)
        }
    )
}

pub fn iter_files(dir: ReadDir) -> impl Iterator<Item = DirEntry> {
    dir.filter_map(Result::ok).filter(|entry| entry.metadata().ok().map_or(false, |m| m.is_file()))
}

fn is_save_file(path: &Path) -> bool { path.extension() == Some(OsStr::from_bytes(b"save")) }

#[cfg(test)]
mod tests {
    #[test]
    fn is_save_file() {
        use std::path::Path;

        assert!(!super::is_save_file(Path::new("/etc/apt/sources.list.d/pop-os-apps.sources")));
        assert!(super::is_save_file(Path::new("/etc/apt/sources.list.d/pop-os-apps.sources.save")));
    }
}
