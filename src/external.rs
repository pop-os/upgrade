use async_process::{Command, Stdio};
use futures::{
    io::{AsyncBufReadExt, BufReader},
    StreamExt,
};
use std::{io, path::Path};

pub async fn findmnt_uuid<P: AsRef<Path>>(path: P) -> io::Result<String> {
    let mut child_process = Command::new("findmnt")
        .stdout(Stdio::piped())
        .args(&["-n", "-o", "UUID"])
        .arg(path.as_ref())
        .spawn()
        .map_err(|why| io::Error::new(io::ErrorKind::NotFound, why))?;

    let reader = BufReader::new(child_process.stdout.take().unwrap());

    if let Some(Ok(line)) = reader.lines().next().await {
        Ok(line)
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, "findmnt: uuid not found for device"))
    }
}
