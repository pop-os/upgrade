use anyhow::Context;
use async_fs::{copy, File};
use std::{io, fs, path::Path};

pub async fn create<P: AsRef<Path>>(path: P) -> io::Result<File> {
    File::create(&path).await.map_err(|why| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("unable to create file at {:?}: {}", path.as_ref(), why),
        )
    })
}

pub async fn open<P: AsRef<Path>>(path: P) -> io::Result<File> {
    File::open(&path).await.map_err(|why| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("unable to open file at {:?}: {}", path.as_ref(), why),
        )
    })
}

pub async fn cp<'a>(src: &'a Path, dst: &'a Path) -> io::Result<u64> {
    copy(src, dst).await.map_err(|why| {
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

pub fn format_error(source: &(dyn std::error::Error + 'static)) -> String {
    let mut out = fomat!((source));

    let mut source = source.source();
    while let Some(why) = source {
        out.push_str(&fomat!(": "(why)));
        source = why.source();
    }

    out
}

pub fn uid_min_max() -> anyhow::Result<(u32, u32)> {
    let login_defs = fs::read_to_string("/etc/login.defs")
        .context("could not read /etc/login.defs")?;

    let defs = whitespace_conf::parse(&login_defs);

    defs.get("UID_MIN")
        .zip(defs.get("UID_MAX"))
        .context("/etc/login.defs does not contain UID_MIN + UID_MAX")
        .and_then(|(min, max)| {
            let min = min.parse::<u32>().context("UID_MIN is not a u32 value")?;
            let max = max.parse::<u32>().context("UID_MAX is not a u32 value")?;
            Ok((min, max))
        })
}
