use anyhow::Context;
use async_fs::{copy, File};
use std::{io, path::Path};

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

pub fn uid_min_max() -> anyhow::Result<(libc::uid_t, libc::uid_t)> {
    let login_defs =
        std::fs::read_to_string("/etc/login.defs").context("could not read /etc/login.defs")?;

    let mut uid_min = None;
    let mut uid_max = None;

    for line in login_defs.lines() {
        let line = line.trim();

        let mut fields = line.split_ascii_whitespace();

        match fields.next() {
            Some("UID_MIN") => {
                uid_min = Some(fields.next().context("could not read UID_MIN value")?)
            }
            Some("UID_MAX") => {
                uid_max = Some(fields.next().context("could not read UID_MAX value")?)
            }
            _ => continue,
        }

        if uid_min.is_some() && uid_max.is_some() {
            break;
        }
    }

    let uid_min = uid_min
        .context("could not find UID_MIN value")?
        .parse::<libc::uid_t>()
        .context("UID_MIN is NaN")?;

    let uid_max = uid_max
        .context("could not find UID_MAX value")?
        .parse::<libc::uid_t>()
        .context("UID_MAX is NaN")?;

    Ok((uid_min, uid_max))
}
