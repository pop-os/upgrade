use super::{apt_hold, errors::ReleaseError};
use std::{
    fs, io,
    os::unix::process::ExitStatusExt,
    process::{Command, Stdio},
};

/// Holds all packages which have a pre-depend on snapd.
///
/// This should be executed after the source lists are upgraded to the new release,
/// and before packages have been fetched.
pub fn hold_transitional_packages() -> Result<(), ReleaseError> {
    let snap_packages =
        fetch_transitional_packages().map_err(ReleaseError::TransitionalSnapFetch)?;

    let mut buffer = String::new();

    for package in &snap_packages {
        buffer.push_str(&*package);
        buffer.push('\n');
    }

    fs::write(crate::TRANSITIONAL_SNAPS, buffer.as_bytes())
        .map_err(ReleaseError::TransitionalSnapRecord)?;

    for package in &snap_packages {
        apt_hold(package).map_err(ReleaseError::TransitionalSnapHold)?;
    }

    Ok(())
}

fn fetch_transitional_packages() -> io::Result<Vec<Box<str>>> {
    let output = check_output("apt-cache", &["rdepends", "snapd"])?;
    let mut transitional = Vec::new();

    for rdepend in output.lines().skip(2) {
        let rdepend = rdepend.trim();
        if has_predepend(rdepend, "snapd")? {
            eprintln!("{} has a pre-depend on snapd", rdepend);
            transitional.push(Box::from(rdepend));
        }
    }

    Ok(transitional)
}

fn has_predepend(package: &str, predepend: &str) -> io::Result<bool> {
    let output = check_output("apt-cache", &["depends", dbg!(package)])?;

    let mut found = false;

    for line in output.lines().skip(1) {
        let line = line.trim();
        if line.starts_with("PreDepends: ") {
            if &line[12..] == predepend {
                found = true;
                break;
            }
        } else {
            break;
        }
    }

    Ok(found)
}

fn check_output(cmd: &str, args: &[&str]) -> io::Result<String> {
    let output = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()?;

    if output.status.success() {
        String::from_utf8(output.stdout).map_err(|_| {
            io::Error::new(io::ErrorKind::Other, format!("{} output was not UTF-8", cmd))
        })
    } else {
        let source = match output.status.code() {
            Some(code) => io::Error::new(
                io::ErrorKind::Other,
                format!("{} exited with status of {}", cmd, code),
            ),
            None => match output.status.signal() {
                Some(signal) => io::Error::new(
                    io::ErrorKind::Other,
                    format!("{} terminated with signal {}", cmd, signal),
                ),
                None => io::Error::new(io::ErrorKind::Other, format!("{} status is unknown", cmd)),
            },
        };

        Err(source)
    }
}
