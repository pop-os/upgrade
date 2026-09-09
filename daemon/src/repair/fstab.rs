//! All code responsible for validating and repair the /etc/fstab file.

use crate::system_environment::SystemEnvironment;
use std::path::Path;
use std::process::Command;
use super::FstabErr;

/// Performs the following Pop-specific actions:
///
/// - Ensures that `/boot/efi` and `/` are mounted.
pub fn repair() -> Result<(), FstabErr> {
    if SystemEnvironment::detect() != SystemEnvironment::Efi {
        return Ok(());
    }

    // Ensure that all devices have been mounted before proceeding.
    mount_required_partitions()
}

/// Ensure that the necessary mount points are mounted.
fn mount_required_partitions() -> Result<(), FstabErr> {
    // Check /proc/mounts for existing mountpoints rather than relying entirely
    // on mount(1), which gets confused by ZFS filesystems (which might be
    // managed by e.g. zfs-mount.service).
    let mounts = proc_mounts::MountList::new().map_err(FstabErr::ProcMounts)?;

    for mount_point in &["/", "/boot/efi"] {
        if let Some(_) = mounts.get_mount_by_dest(mount_point) {
            continue; // Already mounted.
        }
        Command::new("mount")
            .arg(mount_point)
            .status()
            .map_err(FstabErr::MountSpawn)
            .and_then(|status| {
                // 0 means it mounted an unmounted drive.
                // 32 means it was already mounted.
                match status.code() {
                    Some(0) | Some(32) => Ok(()),
                    _ => Err(FstabErr::Mount { mount_point })
                }
            })?;
    }

    Ok(())
}

pub fn append(fstab: &mut String, file_system: &str, mount: &str, fs_type: &str, options: &str, dump: &str, pass: &str) -> bool {
    let new_line = format!("{file_system}  {mount}  {fs_type}  {options}  {dump}  {pass}");
    let mut prev_line = None;
    for line in fstab.lines() {
        let trimmed_line = line.trim_start();
        if trimmed_line.starts_with('#') {
            continue;
        }

        if trimmed_line.starts_with(file_system) {
            if line == new_line {
                return false;
            }

            prev_line = Some(line.to_owned());
            break;
        }
    }

    if let Some(prev_line) = prev_line {
        *fstab = fstab.replacen(&prev_line, &new_line, 1);
    } else {
        if !fstab.ends_with('\n') {
            fstab.push('\n');
        }

        fstab.push_str(&new_line);
        fstab.push('\n');
    }

    true
}

pub fn find_mount_by_source<'a>(fstab: &'a str, device_path: &'_ Path) -> Option<&'a str> {
    for line in fstab.lines() {
        let trimmed_line = line.trim_start();
        if trimmed_line.starts_with('#') {
            continue;
        }

        let mut fields = trimmed_line.split_ascii_whitespace();
        let Some(file_system) = fields.next() else {
            continue;
        };

        if file_system.starts_with('/') {
            if Path::new(file_system) == device_path {
                return Some(line);
            }
        } else if let Some(part_uuid) = file_system.strip_prefix("PARTUUID=") {
            if let Ok(device) =
                Path::new(&["/dev/disk/by-partuuid/", part_uuid].concat()).canonicalize()
                && device == device_path
            {
                return Some(line);
            }
        } else if let Some(uuid) = file_system.strip_prefix("UUID=") {
            if let Ok(device) = Path::new(&["/dev/disk/by-uuid/", uuid].concat()).canonicalize()
                && device == device_path
            {
                return Some(line);
            }
        } else if let Some(label) = file_system.strip_prefix("LABEL=")
            && let Ok(device) =
                Path::new(&["/dev/disk/by-partlabel/", label].concat()).canonicalize()
            && device == device_path
        {
            return Some(line);
        }
    }

    None
}

pub fn find_mount_by_dest<'a>(fstab: &'a str, dest: &'_ str) -> Option<(&'a str, &'a str)> {
    for line in fstab.lines() {
        let trimmed_line = line.trim_start();
        if trimmed_line.starts_with('#') {
            continue;
        }

        let mut fields = trimmed_line.split_ascii_whitespace();
        let Some((file_system, target_dir)) = fields.next().zip(fields.next()) else {
            continue;
        };

        if dest != target_dir {
            continue;
        }

        return Some((line, file_system));
    }

    None
}

pub fn remove_from_tab(tab: &mut String, block: &str) -> bool {
    if let Some(start) = tab.find(block) {
        // Remove a preceeding newline if found.
        let offset = if start > 0 && tab.as_bytes()[start - 1] == b'\n' {
            1
        } else {
            0
        };

        // Find the next newline and delete this line.
        match tab[start..].find('\n') {
            Some(end) => tab.replace_range(start - offset..start + end, ""),
            None => tab.replace_range(start - offset.., ""),
        }

        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    #[test]
    pub fn remove_cryptswap_from_crypttab() {
        let mut sample = String::from(
            "cryptswap UUID=c6c8fd31-4f34-488e-bdad-2079c9dff4a8 /dev/urandom swap,plain,offset=1024,cipher=aes-xts-plain64,size=512",
        );
        assert!(super::remove_from_tab(&mut sample, "cryptswap"));
        assert_eq!(sample.as_str(), "");

        let mut sample = String::from(
            r#"cryptdata UUID=12345678-abcd-1234-abcd-123456789abc none luks
cryptswap UUID=c6c8fd31-4f34-488e-bdad-2079c9dff4a8 /dev/urandom swap,plain,offset=1024,cipher=aes-xts-plain64,size=512"#,
        );

        assert!(super::remove_from_tab(&mut sample, "cryptswap"));
        assert_eq!(
            sample.as_str(),
            "cryptdata UUID=12345678-abcd-1234-abcd-123456789abc none luks"
        );
    }

    #[test]
    pub fn remove_cryptswap_from_fstab() {
        let sample = r#"# <file system>  <mount point>  <type>  <options>  <dump>  <pass>
PARTUUID=6fb88ca3-dbe0-4164-bb58-9c15a3b84e9d  /boot/efi  vfat  umask=0077  0  0
PARTUUID=4717b1ab-af1a-442e-b385-a060366c00a6  /recovery  vfat  umask=0077  0  0
UUID=2a9922b4-f519-49ca-8739-1164d82b5afc  /  ext4  noatime,errors=remount-ro  0  1
/dev/mapper/cryptswap  none  swap  defaults  0  0"#;

        let expected = r#"# <file system>  <mount point>  <type>  <options>  <dump>  <pass>
PARTUUID=6fb88ca3-dbe0-4164-bb58-9c15a3b84e9d  /boot/efi  vfat  umask=0077  0  0
PARTUUID=4717b1ab-af1a-442e-b385-a060366c00a6  /recovery  vfat  umask=0077  0  0
UUID=2a9922b4-f519-49ca-8739-1164d82b5afc  /  ext4  noatime,errors=remount-ro  0  1"#;

        let mut fstab = sample.to_owned();
        assert!(super::remove_from_tab(&mut fstab, "/dev/mapper/cryptswap"));
        assert_eq!(fstab.as_str(), expected);

        let sample = r#"# <file system>  <mount point>  <type>  <options>  <dump>  <pass>
PARTUUID=6fb88ca3-dbe0-4164-bb58-9c15a3b84e9d  /boot/efi  vfat  umask=0077  0  0
PARTUUID=4717b1ab-af1a-442e-b385-a060366c00a6  /recovery  vfat  umask=0077  0  0
/dev/mapper/cryptswap  none  swap  defaults  0  0
UUID=2a9922b4-f519-49ca-8739-1164d82b5afc  /  ext4  noatime,errors=remount-ro  0  1
"#;

        let expected = r#"# <file system>  <mount point>  <type>  <options>  <dump>  <pass>
PARTUUID=6fb88ca3-dbe0-4164-bb58-9c15a3b84e9d  /boot/efi  vfat  umask=0077  0  0
PARTUUID=4717b1ab-af1a-442e-b385-a060366c00a6  /recovery  vfat  umask=0077  0  0
UUID=2a9922b4-f519-49ca-8739-1164d82b5afc  /  ext4  noatime,errors=remount-ro  0  1
"#;
        let mut fstab = sample.to_owned();
        assert!(super::remove_from_tab(&mut fstab, "/dev/mapper/cryptswap"));
        assert_eq!(fstab.as_str(), expected);
    }
}
