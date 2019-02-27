mod upgrade_event;

pub use self::upgrade_event::AptUpgradeEvent;

use crate::status::StatusExt;
use std::io;
use std::process::Command;

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

/// Same as `apt_noninteractive`, but also has a callback for handling lines of output.
fn apt_noninteractive_callback<F: FnMut(&mut Command) -> &mut Command, C: Fn(&str)>(
    mut func: F,
    callback: C,
) -> io::Result<()> {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;

    let mut child = func(
        Command::new("apt-get")
            .env("DEBIAN_FRONTEND", "noninteractive")
            .args(&["-y", "--allow-downgrades"]),
    )
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?;

    let mut buffer = String::new();
    let mut stdout = child.stdout.take().map(BufReader::new);
    let mut stderr = child.stderr.take().map(BufReader::new);

    loop {
        match child.try_wait()? {
            Some(status) => return status.as_result(),
            None => {
                if let Some(ref mut stdout) = stdout {
                    if let Ok(read) = stdout.read_line(&mut buffer) {
                        if read != 0 {
                            eprintln!("stdout: {}", buffer);
                            callback(&buffer);
                            buffer.clear();
                        }
                    }
                }

                if let Some(ref mut stderr) = stderr {
                    if let Ok(read) = stderr.read_line(&mut buffer) {
                        if read != 0 {
                            eprintln!("stderr: {}", buffer);
                            callback(&buffer);
                            buffer.clear();
                        }
                    }
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
