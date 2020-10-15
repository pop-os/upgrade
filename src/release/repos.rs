use crate::repair::sources;
use anyhow::Context;
use os_str_bytes::OsStrBytes;
use std::{
    ffi::OsStr,
    fs::{self, DirEntry, ReadDir},
    path::Path,
};

const BACKUP_MAIN_FILE: &str = "/etc/apt/sources.list.save";
const MAIN_FILE: &str = "/etc/apt/sources.list";
const PPA_DIR: &str = "/etc/apt/sources.list.d";

/// Backup the sources lists
pub fn backup(release: &str) -> anyhow::Result<()> {
    if Path::new(PPA_DIR).exists() {
        // Remove previous backups
        let dir = fs::read_dir(PPA_DIR).context("cannot read PPA directory")?;
        iter_files(dir, |entry| {
            if entry.file_name().to_bytes().ends_with(b".save") {
                let path = entry.path();
                info!("removing old backup at {}", path.display());
                fs::remove_file(&path)
                    .with_context(|| fomat!("failed to remove backup at "(path.display())))?;
            }

            Ok(())
        })?;

        // Create new backups
        let dir = fs::read_dir(PPA_DIR).context("cannot read PPA directory")?;
        iter_files(dir, |entry| {
            let src_path = entry.path();
            let dst_path_buf = [&*(src_path.to_bytes()), b".save"].concat();
            let dst_path_str = OsStr::from_bytes(&dst_path_buf).unwrap();
            let dst_path = Path::new(&dst_path_str);

            info!("creating backup of {} to {}", src_path.display(), dst_path.display());
            fs::copy(&src_path, dst_path).with_context(
                || fomat!("failed to copy " (src_path.display()) " to " (dst_path.display())),
            )?;

            Ok(())
        })?;
    }

    if Path::new(MAIN_FILE).exists() {
        if Path::new(BACKUP_MAIN_FILE).exists() {
            info!("removing old backup at {}", BACKUP_MAIN_FILE);
            fs::remove_file(BACKUP_MAIN_FILE).context("failed to remove backup of sources.list")?;
        }
        info!("creating backup of {} to {}", MAIN_FILE, BACKUP_MAIN_FILE);
        fs::copy(MAIN_FILE, BACKUP_MAIN_FILE)
            .context("failed to copy sources list to backup path")
            .map(|_| ())
    } else {
        info!("sources list was not found — creating a new one");
        sources::create_new_sources_list(release).context("failed to create new sources.list")
    }
}

/// For each `.list` in `sources.list.d`, add `#` to the `deb` lines.
pub fn disable_third_parties() -> anyhow::Result<()> {
    let dir = fs::read_dir(PPA_DIR).context("cannot read PPA directory")?;
    iter_files(dir, |entry| {
        if entry.file_name().to_bytes().ends_with(b".list") {
            let path = entry.path();

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

        Ok(())
    })
}

pub fn repair(release: &str) -> anyhow::Result<()> {
    if !Path::new(MAIN_FILE).exists() {
        sources::create_new_sources_list(release)?;
    }

    Ok(())
}

/// Restore a previous backup of the sources lists
pub fn restore() -> anyhow::Result<()> {
    let dir = fs::read_dir(PPA_DIR).context("cannot read PPA directory")?;
    iter_files(dir, |entry| {
        let src_path = entry.path();
        let src_bytes = src_path.to_bytes();
        if src_bytes.ends_with(b".save") {
            let dst_bytes = &src_bytes[..src_bytes.len() - 5];
            let dst_str = OsStr::from_bytes(dst_bytes).unwrap();
            let dst = Path::new(&dst_str);

            info!("restoring source list at {}", dst.display());

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
        info!("restoring system sources list");
        if Path::new(MAIN_FILE).exists() {
            fs::remove_file(MAIN_FILE).context("failed to remove modified system sources.list")?;
        }

        fs::rename(BACKUP_MAIN_FILE, MAIN_FILE).context("failed to restore system sources.list")?;
    }

    Ok(())
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
