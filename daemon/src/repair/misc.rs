use std::{
    cmp::Ordering,
    fs::{self, File},
    io::{self, Write},
    path::Path,
};

// On the upgrade to 19.10, dkms is unable to build modules for kernels older than 5.3
// because of incompatible options in GCC 9. This can be fixed by removing the
// incompatible options in the `Makefile` of these kernels.
//
// Canonical will be releasing updated kernels, but it is possible that someone upgrading
// from either 18.04 or 19.04 may still have sources for an older kernel.
pub fn dkms_gcc9_fix() -> io::Result<()> {
    const UNAFFECTED: &str = "5.3.0";
    const BADFLAGS: [&str; 2] =
        [" -mindirect-branch=thunk-extern", " -mindirect-branch=thunk-inline"];

    let lib_modules_dir: &'static Path = ["/lib/modules", "/usr/lib/modules"]
        .iter()
        .map(Path::new)
        .find(|path| path.exists())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "A path to the /lib/modules directory does not exist",
            )
        })?;

    for dir in fs::read_dir(lib_modules_dir)?.flatten() {
        if let Some(file_name) = dir.file_name().to_str() {
            if human_sort::compare(file_name, UNAFFECTED) == Ordering::Less {
                let makefile = dir.path().join("build/Makefile");
                if makefile.exists() {
                    let mut data = fs::read_to_string(&makefile)?;
                    let mut file = File::create(&makefile)?;

                    data = data.replace(BADFLAGS[0], "");
                    data = data.replace(BADFLAGS[1], "");
                    file.write_all(data.as_bytes())?;
                }
            }
        }
    }

    Ok(())
}

/// Remove the pulse directory from each user.
pub fn wipe_pulse() -> io::Result<()> {
    for user in pwd::Passwd::iter() {
        let path = Path::new(&*user.dir).join(".config/pulse");
        if path.exists() {
            if path.is_dir() {
                std::fs::remove_dir_all(&path)?;
            } else {
                std::fs::remove_file(&path)?;
            }
        }
    }

    Ok(())
}
