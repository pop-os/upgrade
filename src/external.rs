use command::Command;
use std::io;
use std::path::Path;

// pub fn update_initramfs() -> io::Result<()> {
//     Command::new("update-initramfs").args(&["-ck", "all"]).run()
// }

pub fn rsync(src: &[&Path], target: &str, args: &[&str]) -> io::Result<()> {
    Command::new("rsync").args(args).args(src).arg(target).run()
}

pub fn findmnt_uuid<P: AsRef<Path>>(path: P) -> io::Result<String> {
    let uuid = Command::new("findmnt")
        .args(&["-n", "-o", "UUID"])
        .arg(path.as_ref())
        .run_with_stdout()?;

    match uuid.lines().next() {
        Some(line) => Ok(line.to_owned()),
        None => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "uuid not found for device",
        )),
    }
}
