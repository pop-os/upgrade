mod upgrade_event;

use std::fs::File;
use std::os::unix::io::{IntoRawFd, FromRawFd};
pub use self::upgrade_event::AptUpgradeEvent;

use crate::status::StatusExt;
use libc;
use std::io;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

/// Execute the apt command non-interactively, using whichever additional arguments are provided.
fn apt_noninteractive<F: FnMut(&mut Command) -> &mut Command>(mut func: F) -> io::Result<()> {
    func(
        Command::new("apt-get")
            .env("DEBIAN_FRONTEND", "noninteractive")
            .args(&["-y", "--allow-downgrades"]),
    )
    .status()
    .and_then(StatusExt::as_result)
}

fn non_blocking<F: IntoRawFd>(fd: F) -> File {
    let fd = fd.into_raw_fd();
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL, 0);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        File::from_raw_fd(fd)
    }
}

fn non_blocking_line_reading<B: BufRead, F: Fn(&str)>(which: &str, reader: &mut B, buffer: &mut String, callback: F) -> io::Result<()> {
    loop {
        match reader.read_line(buffer) {
            Ok(0) => break,
            Ok(_read) => {
                eprintln!("{}: {}", which, buffer);
                callback(&buffer);
                buffer.clear();
            }
            Err(ref why) if why.kind() == io::ErrorKind::WouldBlock => {
                break
            }
            Err(why) => return Err(why)
        }
    }

    Ok(())
}

/// Same as `apt_noninteractive`, but also has a callback for handling lines of output.
fn apt_noninteractive_callback<F: FnMut(&mut Command) -> &mut Command, C: Fn(&str)>(
    mut func: F,
    callback: C,
) -> io::Result<()> {


    let mut child = func(
        Command::new("apt-get")
            .env("DEBIAN_FRONTEND", "noninteractive")
            .env("LANG", "C")
            .args(&["-y", "--allow-downgrades"]),
    )
    .stdout(Stdio::piped())
    .spawn()?;

    let mut stdout_buffer = String::new();
    let mut stdout = child.stdout.take()
        .map(non_blocking)
        .map(BufReader::new);

    loop {
        match child.try_wait()? {
            Some(status) => return status.as_result(),
            None => {
                if let Some(ref mut stdout) = stdout {
                    non_blocking_line_reading("stdout", stdout, &mut stdout_buffer, &callback)?;
                }
            }
        }
    }
}

/// apt-get -y --allow-downgrades full-upgrade
pub fn apt_update() -> io::Result<()> {
    apt_noninteractive(|cmd| cmd.arg("update"))
}

/// apt-get -y --allow-downgrades full-upgrade
pub fn apt_upgrade(callback: &mut dyn Fn(AptUpgradeEvent)) -> io::Result<()> {
    apt_noninteractive_callback(
        |cmd| cmd.args(&["--show-progress", "full-upgrade"]),
        move |line| {
            if let Ok(event) = line.parse::<AptUpgradeEvent>() {
                callback(event);
            }
        },
    )
}

/// apt-get -y --allow-downgrades install
pub fn apt_install(packages: &[&str]) -> io::Result<()> {
    apt_noninteractive(move |cmd| cmd.arg("install").args(packages))
}

/// dpkg --configure -a
pub fn dpkg_configure_all() -> io::Result<()> {
    // TODO: progress callback support.
    Command::new("dpkg").args(&["--configure", "-a"]).status().and_then(StatusExt::as_result)
}
