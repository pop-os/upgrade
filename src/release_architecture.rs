use std::io;
use sysfs_class::{PciDevice, SysClass};

#[derive(Debug, Error)]
pub enum ReleaseArchError {
    #[error(display = "error when probing PCI device: {}", _0)]
    PciProbe(io::Error),
    #[error(display = "error fetching vendor ID of PCI device: {}", _0)]
    PciVendor(io::Error),
}

/// Probe PCI devices for the existence of NVIDIA hardware, and return either "intel" or "nvidia".
pub fn detect_arch() -> Result<&'static str, ReleaseArchError> {
    const VID_NVIDIA: u16 = 0x10DE;

    for device in PciDevice::iter() {
        let device = device.map_err(ReleaseArchError::PciProbe)?;
        if device.vendor().map_err(ReleaseArchError::PciVendor)? == VID_NVIDIA {
            return Ok("nvidia");
        }
    }

    Ok("intel")
}
