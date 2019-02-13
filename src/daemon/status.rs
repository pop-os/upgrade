#[repr(u8)]
#[derive(Copy, Clone, Debug, FromPrimitive, PartialEq)]
pub enum DaemonStatus {
    Inactive = 0,
    FetchingPackages = 1,
    RecoveryUpgrade = 2,
    ReleaseUpgrade = 3,
}

impl From<DaemonStatus> for &'static str {
    fn from(status: DaemonStatus) -> Self {
        match status {
            DaemonStatus::Inactive => "inactive",
            DaemonStatus::FetchingPackages => "fetching package updates",
            DaemonStatus::RecoveryUpgrade => "upgrading recovery partition",
            DaemonStatus::ReleaseUpgrade => "upgrading distribution release",
        }
    }
}
