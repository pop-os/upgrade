//! All code responsible for validating and repair the /etc/fstab file.

use self::FileSystem::*;
use crate::system_environment::SystemEnvironment;
use as_result::MapResult;
use distinst_disks::{BlockDeviceExt, Disks, FileSystem, PartitionExt, PartitionInfo};
use partition_identity::{PartitionID, PartitionSource};
use proc_mounts::{MountInfo, MountIter, MountTab};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FstabError {
    #[error("failed to create backup of original fstab")]
    BackupCreate(#[source] io::Error),

    #[error(
        "failed to restore backup of original fstab: {}: originally caused by: {}",
        why,
        original
    )]
    BackupRestore { why: io::Error, original: Box<FstabError> },

    #[error("failed to open the fstab file for writing")]
    Create(#[source] io::Error),

    #[error("failed to probe disk for missing mount point")]
    DiskProbe(#[source] io::Error),

    #[error("source in fstab has an invalid ID: '{}", _0)]
    InvalidSourceId(String),

    #[error("failed to create missing directory at {}", path)]
    MissingDirCreation { path: &'static str, source: io::Error },

    #[error("failed to mount devices with `mount -a`")]
    MountFailure(#[source] io::Error),

    #[error("failed to parse the fstab file")]
    Parse(#[source] io::Error),

    #[error("failed to read /proc/mounts")]
    ProcRead(#[source] io::Error),

    #[error("failed to read the fstab file")]
    Read(#[source] io::Error),

    #[error("root partition's device path was not found (maybe it is a logical device?)")]
    RootDeviceNotFound,

    #[error("root partition was not found in the fstab file")]
    RootNotFound,

    #[error("not only is root not listed in the fstab, it's also not mounted")]
    RootNotMounted,

    #[error("failed to find the source ID by its path: {:?}", _0)]
    SourceNotFound(PathBuf),

    #[error("failed to find a device path for a fstab source ID: {:?}", _0)]
    SourceWithoutDevice(PathBuf),

    #[error("failed to find either a PartUUID or UUID for a fstab source: {:?}", _0)]
    SourceWithoutIDs(PathBuf),

    #[error("failed to write to fstab")]
    Write(io::Error),
}

/// Performs the following Pop-specific actions:
///
/// - Ensures that `/boot/efi` and `/recovery` are mounted by PartUUID.
/// - If the `/recovery` mount is missing, find it.
/// - If the `/recovery` partition is not mounted, mount it.
pub fn repair() -> Result<(), FstabError> {
    if SystemEnvironment::detect() != SystemEnvironment::Efi {
        return Ok(());
    }

    let fstab = fs::read_to_string("/etc/fstab").map_err(FstabError::Read)?;

    let mount_tab = &mut fstab.parse::<MountTab>().map_err(FstabError::Parse)?;

    const EFI: &str = "/boot/efi";
    const RECOVERY: &str = "/recovery";

    // Create missing mount directories, if the mounts are missing.
    for path in &[EFI, RECOVERY] {
        if !Path::new(*path).exists() {
            fs::create_dir(*path)
                .map_err(|source| FstabError::MissingDirCreation { path: *path, source })?;
        }
    }

    let (root_id, (found_efi, efi_id), (found_recovery, recovery_id)) = {
        let mut root = None;
        let mut efi = None;
        let mut recovery = None;

        for mount in mount_tab.iter_mounts() {
            let dest = mount.dest.as_path();

            if dest == Path::new(RECOVERY) {
                recovery = Some(mount);
            } else if dest == Path::new(EFI) {
                efi = Some(mount);
            } else if dest == Path::new("/") {
                root = Some(mount);
            }

            if root.is_some() && efi.is_some() && recovery.is_some() {
                break;
            }
        }

        (fstab_check_root(root)?, fstab_fix_source(efi)?, fstab_fix_source(recovery)?)
    };

    for (target, source) in &[("/", root_id), (EFI, efi_id), (RECOVERY, recovery_id)] {
        if let Some(ref source) = *source {
            let fstype: String = if *target == "/" {
                let mtab =
                    MountIter::new_from_file("/proc/mounts").map_err(FstabError::ProcRead)?;

                let root_mount = mtab
                    .flat_map(Result::ok)
                    .find(|e| e.dest == Path::new("/"))
                    .ok_or_else(|| FstabError::RootNotMounted)?;

                root_mount.fstype
            } else {
                "vfat".into()
            };

            let info = MountInfo {
                source: PathBuf::from(format!("{}", source)),
                dest: PathBuf::from(target),
                fstype,
                // The findmnt command from util-linux errors if root is not set to 1
                pass: 1,
                ..Default::default()
            };

            fstab_insert(mount_tab, target, info)?;
        }
    }

    // If we are in an EFI environment, and the EFI partition was not found, we need to add it.
    if !found_efi {
        fstab_find(
            mount_tab,
            EFI,
            |fs| fs == Fat16 || fs == Fat32,
            |_, path| path.join("EFI").exists(),
        )?;
    }

    // If the recovery partition was not found, find it and mount it. It's okay if the partition
    // is not found, as many people may not have a recovery partition.
    if !found_recovery {
        let result = fstab_find(
            mount_tab,
            RECOVERY,
            |fs| fs == Fat16 || fs == Fat32,
            |_, path| path.join("recovery.conf").exists(),
        );

        match result {
            Ok(()) => (),
            Err(FstabError::DiskProbe(ref why)) if why.kind() == io::ErrorKind::NotFound => (),
            Err(why) => return Err(why),
        }
    }

    fstab_write(&mount_tab)?;

    // Ensure that all devices have been mounted before proceeding.
    mount_all().map_err(FstabError::MountFailure)
}

fn mount_all() -> io::Result<()> {
    std::process::Command::new("mount").arg("-a").status().map_result()
}

fn fstab_check_root(root: Option<&MountInfo>) -> Result<Option<PartitionID>, FstabError> {
    let root = root.ok_or(FstabError::RootNotFound)?;
    let root = root.source.to_str().expect("root partition has a source entry which is not UTF-8");

    let mut root_id =
        root.parse::<PartitionID>().map_err(|_| FstabError::InvalidSourceId(root.to_owned()))?;

    if root_id.variant != PartitionSource::UUID {
        root_id = root_id.get_device_path().ok_or(FstabError::RootDeviceNotFound).and_then(
            |ref path| {
                PartitionID::get_source(PartitionSource::UUID, path)
                    .ok_or_else(|| FstabError::SourceWithoutDevice(PathBuf::from(path)))
            },
        )?;

        return Ok(Some(root_id));
    }

    Ok(None)
}

/// Ensure that a mount in the fstab is mounted by PartUUID.
///
/// Returns `Ok(false)` if the mount was not found.
fn fstab_fix_source(mount: Option<&MountInfo>) -> Result<(bool, Option<PartitionID>), FstabError> {
    let mut id_modification = None;

    // If the mount was found, ensure that it has the correct identifier.
    if let Some(mount) = mount {
        // If the mount partition is not mounted via PartUUID, change it to do precisely that.
        let source = mount.source.to_str().expect("device path with non-UTF8 source");

        let source_id = source
            .parse::<PartitionID>()
            .map_err(|_| FstabError::InvalidSourceId(source.to_owned()))?;

        if source_id.variant != PartitionSource::PartUUID {
            let source_device = source_id
                .get_device_path()
                .ok_or_else(|| FstabError::SourceWithoutDevice(PathBuf::from(source)))?;

            let id = PartitionID::get_source(PartitionSource::PartUUID, &source_device)
                .ok_or_else(|| FstabError::SourceNotFound(source_device.to_owned()))?;

            id_modification = Some(id);
        }

        return Ok((true, id_modification));
    }

    Ok((false, id_modification))
}

fn fstab_find<E, S, C>(
    buffer: &mut MountTab,
    expected_at: E,
    supported: S,
    condition: C,
) -> Result<(), FstabError>
where
    E: AsRef<Path>,
    S: FnMut(FileSystem) -> bool,
    C: FnMut(&PartitionInfo, &Path) -> bool,
{
    // In case it was not found, probe for the location of the mount.
    let expected_at = expected_at.as_ref();
    let result = Disks::probe_for(
        expected_at,
        supported,
        condition,
        move |partition, _mount| -> Result<(), FstabError> {
            let path = partition.get_device_path();
            let id = PartitionID::get_source(PartitionSource::PartUUID, path)
                .ok_or_else(|| FstabError::SourceWithoutDevice(path.to_owned()))?;

            let fs = match partition.get_file_system() {
                Some(fs) => match fs {
                    Fat16 | Fat32 => "vfat",
                    _ => fs.into(),
                },
                None => "none",
            };

            let mount_info = MountInfo {
                source: PathBuf::from(format!("{}", id)),
                dest: expected_at.to_path_buf(),
                fstype: fs.into(),
                ..Default::default()
            };

            fstab_insert(buffer, expected_at, mount_info)
        },
    );

    result.map_err(FstabError::DiskProbe)?
}

fn fstab_insert<P: AsRef<Path>>(
    buffer: &mut MountTab,
    dest: P,
    id: MountInfo,
) -> Result<(), FstabError> {
    let dest = dest.as_ref();
    for mount in buffer.iter_mounts_mut() {
        if mount.dest == dest {
            mount.source = id.source;
            return Ok(());
        }
    }

    buffer.push(id);

    Ok(())
}

fn fstab_write(buffer: &MountTab) -> Result<(), FstabError> {
    fn write(buffer: &MountTab) -> Result<(), FstabError> {
        let file = &mut fs::File::create("/etc/fstab").map_err(FstabError::Create)?;
        write!(file, "{}", buffer).map_err(FstabError::Write)
    }

    const ORIG: &str = "/etc/fstab";
    const BACK: &str = "/etc/fstab.bak";

    fs::copy(ORIG, BACK).map_err(FstabError::BackupCreate)?;
    if let Err(cause) = write(buffer) {
        if let Err(why) = fs::copy(BACK, ORIG) {
            return Err(FstabError::BackupRestore { why, original: Box::new(cause) });
        }

        return Err(cause);
    }

    Ok(())
}
