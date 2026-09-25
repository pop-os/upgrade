use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::fs::Permissions;
use super::{EspErr as Error, fstab, FSTAB_PATH};
use crate::process::exec;

const CRYPTSWAP_PATH: &str = "/dev/mapper/cryptswap";
const CRYPTTAB_PATH: &str = "/etc/crypttab";
const EFI_PART_TYPE: &str = "C12A7328-F81F-11D2-BA4B-00A0C93EC93B";
const EFI_PATH: &str = "/boot/efi";

pub type BlockPath = String;
pub type PartPath = String;
pub type PartNumber = String;

/// Upgrade old EFI partition to the 4 GB swap partition if it exists.
pub fn convert_swap() -> Result<(), Error> {
    let mount_list = proc_mounts::MountList::new().map_err(Error::ProcMounts)?;
    let swap_list = proc_mounts::SwapList::new().map_err(Error::ProcSwaps)?;
    let old_esp_part_path = &mount_list
        .get_mount_by_dest(EFI_PATH)
        .ok_or(Error::EspNotFound)?
        .source;

    let mut fstab = std::fs::read_to_string(FSTAB_PATH).map_err(Error::FstabRead)?;
    let old_esp_fstab_line =
        fstab::find_mount_by_source(&fstab, old_esp_part_path).map(String::from);

    let cryptswap_device = Path::new(CRYPTSWAP_PATH);
    let (esp_disk_path, new_esp_part_path, new_esp_part_name) = if let Ok(partition_name) =
        std::env::var("ESP_DEVICE")
    {
        partition_info_by_name(&partition_name).ok_or(Error::EspEnvNotFound)?
    } else if let Ok(cryptswap_dm_path) = cryptswap_device.canonicalize() {
        let info = partition_info_from_dm(&cryptswap_dm_path).ok_or(Error::CryptswapPartitionNotFound)?;
        
        // Check if swap partition is enabled and disable it if so.
        if swap_list.get_swapped(&cryptswap_dm_path) {
            _ = exec(Command::new("swapoff").arg(&cryptswap_dm_path));
        }

        udevadm_settle();

        if cryptswap_device.exists() {
            exec(
                Command::new("cryptsetup")
                    .arg("close")
                    .arg(cryptswap_device),
            )
            .map_err(Error::CryptswapClose)?;
        }

        if fstab::remove_from_tab(&mut fstab, CRYPTSWAP_PATH) {
            crate::fs::atomic_overwrite(Path::new(FSTAB_PATH), fstab.as_bytes())
                .map_err(Error::FstabWrite)?;
        }

        {
            let mut tab = std::fs::read_to_string(CRYPTTAB_PATH).map_err(Error::CrypttabRead)?;

            if fstab::remove_from_tab(&mut tab, "cryptswap") {
                crate::fs::atomic_overwrite(Path::new(CRYPTTAB_PATH), tab.as_bytes())
                    .map_err(Error::CrypttabWrite)?;
            }
        }

        info
    } else {
        // No cryptswap device found, therefore no swap to convert.
        return Ok(());
    };

    // Don't act if the new ESP is the same as the current one.
    if old_esp_part_path == Path::new(&new_esp_part_path) {
        return Ok(());
    }

    let Ok(temp_dir) = tempdir::TempDir::new("pop-esp") else {
        return Err(Error::TempDir);
    };

    udevadm_settle();

    exec(Command::new("wipefs").args(["-a", &new_esp_part_path])).map_err(Error::SwapWipe)?;

    exec(Command::new("mkfs.fat").args(["-F32", &new_esp_part_path])).map_err(Error::EspFormat)?;

    exec(Command::new("sfdisk").args([
        "--part-type",
        &esp_disk_path,
        &new_esp_part_name,
        EFI_PART_TYPE,
    ]))
    .map_err(Error::EspAssignPartType)?;

    udevadm_settle();

    let Some(new_esp_uuid) = partition_part_uuid(Path::new(&new_esp_part_path)) else {
        return Err(Error::EspGetPartUuid);
    };

    let temp_mount = sys_mount::Mount::builder()
        .fstype("vfat")
        .mount_autodrop(
            &new_esp_part_path,
            temp_dir.path(),
            sys_mount::UnmountFlags::DETACH,
        )
        .map_err(Error::EspMountNewTemp)?;

    _ = std::fs::set_permissions(temp_dir.path(), Permissions::from_mode(0o700));

    exec(
        Command::new("rsync")
            .args(["-ap", EFI_PATH])
            .arg(temp_dir.path()),
    )
    .map_err(Error::EspCopy)?;

    exec(
        Command::new("bootctl")
            .args(["install", "--esp-path"])
            .arg(temp_dir.path()),
    )
    .map_err(Error::EspBootctlInstall)?;

    if let Some(line) = old_esp_fstab_line {
        fstab = fstab.replacen(
            &line,
            &format!("PARTUUID={new_esp_uuid}  /boot/efi  vfat  umask=0077  0  0"),
            1,
        );
        crate::fs::atomic_overwrite(Path::new(FSTAB_PATH), fstab.as_bytes())
            .map_err(Error::FstabWrite)?;
    } else if let Some((line, source)) = fstab::find_mount_by_dest(&fstab, EFI_PATH)
        && source != format!("PARTUUID={new_esp_uuid}")
    {
        fstab = fstab.replacen(
            line,
            &format!("PARTUUID={new_esp_uuid}  /boot/efi  vfat  umask=0077  0  0"),
            1,
        );
        crate::fs::atomic_overwrite(Path::new(FSTAB_PATH), fstab.as_bytes())
            .map_err(Error::FstabWrite)?;
    }

    sys_mount::unmount(EFI_PATH, sys_mount::UnmountFlags::DETACH).map_err(Error::EspUnmountOld)?;

    drop(temp_mount);
    drop(temp_dir);

    udevadm_settle();

    _ = std::fs::set_permissions(EFI_PATH, Permissions::from_mode(0o700));

    sys_mount::Mount::builder()
        .fstype("vfat")
        .mount(&new_esp_part_path, EFI_PATH)
        .map_err(Error::EspMountNew)?;

    _ = std::fs::set_permissions(EFI_PATH, Permissions::from_mode(0o700));

    udevadm_settle();

    exec(Command::new("update-initramfs").args(["-ck", "all"])).map_err(Error::InitramfsUpdate)?;

    let unused_device = old_esp_part_path.to_owned();
    std::thread::spawn(move || {
        _ = Command::new("wipefs").arg("-a").arg(unused_device).status();
    });

    Ok(())
}

/// Wait for block devices to settle before continuing.
fn udevadm_settle() {
    _ = Command::new("udevadm").arg("settle").status();
}

/// Get the partition number of a block device if it's a partition.
fn partition_number(block_name: &str) -> Option<String> {
    let path = ["/sys/class/block/", block_name, "/partition"].concat();
    let mut number = std::fs::read_to_string(&path).ok();
    if let Some(ref mut number) = number
        && number.ends_with('\n')
    {
        number.pop();
    }
    number
}

// Get the UUID of a block device by its /dev/ path.
fn partition_part_uuid(block_path: &Path) -> Option<String> {
    partition_id(block_path, "/dev/disk/by-partuuid")
}

fn partition_id(block_path: &Path, by_path: &str) -> Option<String> {
    for block in std::fs::read_dir(by_path).ok()?.filter_map(Result::ok) {
        if let Ok(this_block_path) = block.path().canonicalize()
            && this_block_path == block_path
        {
            return block.file_name().to_str().map(ToOwned::to_owned);
        }
    }

    None
}

/// Get the parent block device of the given block device.
///
/// Identifies `nvme0n1p4` as a partition of `/dev/nvme0n1`.
fn block_device_parent(name: &str) -> Option<BlockPath> {
    for block in std::fs::read_dir("/sys/block").ok()?.filter_map(Result::ok) {
        let block_name = block.file_name();
        let Some(block_name) = block_name.to_str() else {
            continue;
        };

        if !block_name.starts_with("loop")
            && Path::new(&["/sys/block/", block_name, "/", name].concat()).exists()
        {
            return Some(["/dev/", block_name].concat());
        }
    }

    None
}

/// Get the block device that a device map is mapped from.
///
/// Identifies `/dev/mapper/cryptswap` as being mapped from `/dev/nvme0n1p4` on `/dev/nvme0n1`.
fn partition_info_from_dm(device_map: &Path) -> Option<(BlockPath, PartPath, PartNumber)> {
    let device_map_name = device_map.file_name()?.to_str()?;
    let parents = ["/sys/block/", device_map_name, "/slaves"].concat();
    let parent_name = std::fs::read_dir(&parents).ok()?.next()?.ok()?.file_name();
    partition_info_by_name(parent_name.to_str()?)
}

/// Takes `nvme0n1p4` and returns `("/dev/nvme0n1", "/dev/nvme0n1p4", "4")
fn partition_info_by_name(name: &str) -> Option<(BlockPath, PartPath, PartNumber)> {
    let partition_number = partition_number(name)?;
    block_device_parent(name)
        .map(|esp_disk_path| (esp_disk_path, ["/dev/", name].concat(), partition_number))
}
