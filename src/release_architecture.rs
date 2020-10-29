use std::io;
use sysfs_class::{PciDevice, SysClass};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReleaseArchError {
    #[error("error when probing PCI device")]
    PciProbe(#[source] io::Error),

    #[error("error fetching vendor ID of PCI device")]
    PciVendor(#[source] io::Error),
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
