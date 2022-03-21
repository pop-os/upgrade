use std::{io, path::Path, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

pub async fn findmnt_uuid<P: AsRef<Path>>(path: P) -> io::Result<String> {
    let mut cmd = cascade::cascade! {
        Command::new("findmnt");
        ..stdout(Stdio::piped());
        ..args(&["-n", "-o", "UUID"]);
        ..arg(path.as_ref());
    };

    let mut child = cmd.spawn().map_err(|why| io::Error::new(io::ErrorKind::NotFound, why))?;

    let reader = BufReader::new(child.stdout.take().unwrap());

    reader.lines().next_line().await.ok().flatten().map_or_else(
        || Err(io::Error::new(io::ErrorKind::NotFound, "findmnt: uuid not found for device")),
        Ok,
    )
}
