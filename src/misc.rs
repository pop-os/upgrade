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

pub fn format_build_number(value: Option<u16>, buffer: &mut String) -> &str {
    match value {
        None => "false",
        Some(a) => {
            *buffer = format!("{}", a);
            buffer.as_str()
        }
    }
}
