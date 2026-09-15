pub mod crypttab;
pub mod esp;
pub mod fstab;
pub mod misc;
pub mod packaging;
pub mod swapfile;

use crate::{process::CommandErr, system_environment::SystemEnvironment};
use std::io;

const FSTAB_PATH: &str = "/etc/fstab";

error_set::error_set! {
    RepairError := {
        #[display("failed to correct errors in crypttab")]
        Crypttab(CrypttabErr),

        #[display("unable to apply dkms gcc9 fix")]
        DkmsGcc9(io::Error),

        #[display("failed to upgrade EFI partition")]
        Esp(EspErr),

        #[display("error checking and fixing fstab")]
        Fstab(FstabErr),

        #[display("version is not an ubuntu codename: {version}")]
        InvalidVersion { version: String },

        #[display("packaging error")]
        Packaging(packaging::Error),

        #[display("failed to update swapfile")]
        Swapfile(SwapfileErr),

        #[display("failed to wipe pulseaudio settings for users")]
        WipePulse(io::Error),
    }

    CrypttabErr := {
        #[display("failed to read from /etc/crypttab")]
        CrypttabRead(io::Error),
        #[display("failed to write to /etc/crypttab")]
        CrypttabWrite(crate::fs::Error),
    }

    FstabErr := {
        #[display("failed to mount {mount_point}")]
        Mount { mount_point: &'static str },
        #[display("failed to spawn `mount` command")]
        MountSpawn(io::Error),
        #[display("failed to read from /etc/fstab")]
        FstabRead(io::Error),
        #[display("failed to write to /etc/fstab")]
        FstabWrite(crate::fs::Error),
        #[display("could not get mounted devices from /proc/mounts")]
        ProcMounts(io::Error),
    }

    EspErr := {
        #[display("failed to close encrypted swap partition")]
        CryptswapClose(CommandErr),
        #[display("failed to disable encrypted swap partition")]
        CryptswapDisable(CommandErr),
        #[display("could not find cryptswap's partition")]
        CryptswapPartitionNotFound,
        #[display("failed to assign EFI part type to new EFI partition")]
        EspAssignPartType(CommandErr),
        #[display("failed to install systemd-boot to new EFI partition")]
        EspBootctlInstall(CommandErr),
        #[display("failed to copy old ESP data to new EFI partition")]
        EspCopy(CommandErr),
        #[display("defined ESP_DEVICE cannot be found")]
        EspEnvNotFound,
        #[display("failed to get PartUUID of EFI partition")]
        EspGetPartUuid,
        #[display("failed to get UUID of EFI partition")]
        EspGetUuid,
        #[display("new EFI partition failed to mount to a temporary directory")]
        EspMountNewTemp(io::Error),
        #[display("the EFI partition was not found at /boot/efi/")]
        EspNotFound,
        #[display("failed to unmount old EFI partition")]
        EspUnmountOld(io::Error),
        #[display("failed to mount new EFI partition")]
        EspMountNew(io::Error),
        #[display("new EFI partition is missing a UUID")]
        EspUuid,
        #[display("failed to format new EFI partition")]
        EspFormat(CommandErr),
        #[display("failed to update initramfs")]
        InitramfsUpdate(CommandErr),
        #[display("could not get swap devices from /proc/swaps")]
        ProcSwaps(io::Error),
        #[display("failed to wipe file system from swap partition")]
        SwapWipe(CommandErr),
        #[display("could not create a temporary directory for mounting")]
        TempDir,
    } || CrypttabErr || FstabErr

    SwapfileErr := {
        #[display("failed to allocate swapfile")]
        Allocate(rustix::io::Errno),
        #[display("failed to create swapfile")]
        Create(io::Error),
        #[display("failed to enable swapfile")]
        Enable(CommandErr),
        #[display("failed to format swapfile")]
        Format(CommandErr),
        #[display("invalid root block size: {size}")]
        RootBlockSizeInvalid { size: i64 },
        #[display("failed to get file system stats from root file system")]
        RootFsStats(rustix::io::Errno),
    } || FstabErr
}

pub async fn repair() -> Result<(), RepairError> {
    info!("performing release repair");

    let release = &os_release::OS_RELEASE.as_ref().unwrap().version_codename;

    crypttab::repair()?;
    fstab::repair()?;

    if SystemEnvironment::detect() == SystemEnvironment::Efi {
        esp::convert_swap()?;
    }
    
    swapfile::create()?;
    packaging::repair(release).await?;

    Ok(())
}

pub fn pre_upgrade() -> Result<(), RepairError> {
    misc::dkms_gcc9_fix().map_err(RepairError::DkmsGcc9)?;
    misc::wipe_pulse().map_err(RepairError::WipePulse)
}
