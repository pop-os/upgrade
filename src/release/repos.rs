use crate::repair::sources;
use anyhow::Context;
use std::{fs, path::Path};

const BACKUP_DIR: &str = "/etc/apt/sources.list.bak/";
const BACKUP_MAIN_FILE: &str = "/etc/apt/sources.list.bak/list";
const BACKUP_PPA_DIR: &str = "/etc/apt/sources.list.bak/ppas/";
const MAIN_FILE: &str = "/etc/apt/sources.list";
const PPA_DIR: &str = "/etc/apt/sources.list.d";

/// Backup the sources lists
pub fn backup() -> anyhow::Result<()> {
    if Path::new(BACKUP_DIR).exists() {
        fs::remove_dir_all(BACKUP_DIR)
            .context("failed to remove previous backup of repositories")?;
    }

    fs::create_dir_all(BACKUP_DIR).context("failed to create directory for source list backups")?;

    if Path::new(PPA_DIR).exists() {
        fs::rename(PPA_DIR, BACKUP_PPA_DIR).context("failed to move PPAs to backup dir")?;
    }

    fs::rename(MAIN_FILE, BACKUP_MAIN_FILE).context("failed to move sources list to backup dir")?;

    Ok(())
}

pub fn repair(release: &str) -> anyhow::Result<()> {
    if !Path::new(MAIN_FILE).exists() {
        sources::create_new_sources_list(release)?;
    }

    Ok(())
}

/// Restore a previous backup of the soruces lists
pub fn restore() -> anyhow::Result<()> {
    if Path::new(BACKUP_PPA_DIR).exists() {
        if Path::new(PPA_DIR).exists() {
            fs::remove_dir_all(PPA_DIR).context("failed to remove PPA directory")?;
        }

        fs::rename(BACKUP_PPA_DIR, PPA_DIR).context("failed to restore PPA directory")?;
    }

    if Path::new(BACKUP_MAIN_FILE).exists() {
        if Path::new(MAIN_FILE).exists() {
            fs::remove_file(MAIN_FILE).context("failed to remove sources list")?;
        }

        fs::rename(BACKUP_MAIN_FILE, MAIN_FILE).context("failed to restore sources list")?;
    }

    Ok(())
}
