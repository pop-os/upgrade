use distinst_chroot::Command;
use std::{io, path::Path};

pub fn findmnt_uuid<P: AsRef<Path>>(path: P) -> io::Result<String> {
    let uuid =
        Command::new("findmnt").args(&["-n", "-o", "UUID"]).arg(path.as_ref()).run_with_stdout()?;

    match uuid.lines().next() {
        Some(line) => Ok(line.to_owned()),
        None => Err(io::Error::new(io::ErrorKind::NotFound, "uuid not found for device")),
    }
}
