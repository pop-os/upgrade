use std::{fs::metadata, io, os::linux::fs::MetadataExt, time::SystemTime};

/// The time at which this OS was installed, as seconds since the Unix Epoch.
pub fn time() -> io::Result<i64> { metadata("/etc/machine-id").map(|md| md.st_ctime()) }

/// The number of seconds since the install date.
pub fn since() -> io::Result<i64> { time().map(|ctime| current() as i64 - ctime) }

/// Time since the Unix Epoch, in this moment.
pub fn current() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
