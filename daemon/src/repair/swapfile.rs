use super::SwapfileErr as Error;
use crate::process::exec;
use rustix::fs::FallocateFlags;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::Command;

const SWAPFILE_PATH: &str = "/swapfile";
const MEBIBYTE: u64 = 1_048_576;

pub fn create() -> Result<(), Error> {
    let fs_stats = rustix::fs::statfs("/").map_err(Error::RootFsStats)?;
    let Ok(block_size) = u64::try_from(fs_stats.f_bsize) else {
        return Err(Error::RootBlockSizeInvalid {
            size: fs_stats.f_bsize,
        })?;
    };

    let available_mib = fs_stats.f_bavail * block_size / MEBIBYTE;

    // Up to 40% of total physical memory.
    let swapfile_capacity = {
        let mut sysinfo = sysinfo::System::new();
        sysinfo.refresh_memory();
        let max_swapfile = sysinfo.total_memory() / MEBIBYTE * 10 / 25;
        let disk_limit = if available_mib < 20480 {
            return Ok(());
        } else {
            available_mib - 20480
        };

        max_swapfile.min(disk_limit).max(4096) * MEBIBYTE
    };

    if let Ok(existing_swapfile) = std::fs::metadata(SWAPFILE_PATH) {
        if existing_swapfile.len() == swapfile_capacity {
            return Ok(());
        }

        _ = Command::new("swapoff").arg(SWAPFILE_PATH).status();
    }

    _ = std::fs::remove_file(SWAPFILE_PATH);

    {
        let swapfile = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(SWAPFILE_PATH)
            .map_err(Error::Create)?;

        rustix::fs::fallocate(&swapfile, FallocateFlags::empty(), 0, swapfile_capacity)
            .map_err(Error::Allocate)?;
    }

    exec(Command::new("mkswap").args(["-U", "clear", SWAPFILE_PATH])).map_err(Error::Format)?;
    exec(Command::new("swapon").arg(SWAPFILE_PATH)).map_err(Error::Enable)?;

    let mut fstab = std::fs::read_to_string(super::FSTAB_PATH).map_err(Error::FstabRead)?;

    if super::fstab::append(&mut fstab, "/swapfile", "none", "swap", "sw", "0", "0") {
        crate::fs::atomic_overwrite(Path::new(super::FSTAB_PATH), fstab.as_bytes())
            .map_err(Error::FstabWrite)?;
    }

    Ok(())
}
