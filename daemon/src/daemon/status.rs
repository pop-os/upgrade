use std::fmt::{self, Display};

#[repr(u8)]
#[derive(Copy, Clone, Debug, FromPrimitive, PartialEq)]
pub enum DaemonStatus {
    Inactive = 0,
    FetchingPackages = 1,
    RecoveryUpgrade = 2,
    ReleaseUpgrade = 3,
    PackageUpgrade = 4,
}

unsafe impl bytemuck::NoUninit for DaemonStatus {}

impl From<DaemonStatus> for &'static str {
    fn from(status: DaemonStatus) -> Self {
        match status {
            DaemonStatus::Inactive => "inactive",
            DaemonStatus::FetchingPackages => "fetching package updates",
            DaemonStatus::RecoveryUpgrade => "upgrading recovery partition",
            DaemonStatus::ReleaseUpgrade => "upgrading distribution release",
            DaemonStatus::PackageUpgrade => "upgrading packages",
        }
    }
}

impl Display for DaemonStatus {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.write_str(<&'static str>::from(*self))
    }
}
