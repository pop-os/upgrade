use anyhow::Context;
use chrono::{DateTime, FixedOffset};
use std::{fs::File, io, path::Path};

pub fn create<P: AsRef<Path>>(path: P) -> io::Result<File> {
    File::create(&path).map_err(|why| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("unable to create file at {:?}: {}", path.as_ref(), why),
        )
    })
}

pub fn open<P: AsRef<Path>>(path: P) -> io::Result<File> {
    File::open(&path).map_err(|why| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("unable to open file at {:?}: {}", path.as_ref(), why),
        )
    })
}

pub fn cp(src: &Path, dst: &Path) -> io::Result<u64> {
    io::copy(&mut open(src)?, &mut create(dst)?).map_err(|why| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("failed to copy {:?} to {:?}: {}", src, dst, why),
        )
    })
}

pub fn format_build_number(value: i16, buffer: &mut String) -> &str {
    if value < 0 {
        "false"
    } else {
        *buffer = format!("{}", value);
        buffer.as_str()
    }
}

pub fn parse_rfc2822(time: &str) -> anyhow::Result<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc2822(time)
        .with_context(|| fomat!("failed to parse RFC 2822 date (" (time) ")"))
}
