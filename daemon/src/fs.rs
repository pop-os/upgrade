use std::io::{self, Write};
use std::path::{Path, PathBuf};

error_set::error_set! {
    #[display("atomic replace of {} failed", path.display())]
    struct Error { source: ErrorKind, path: PathBuf }

    ErrorKind := {
        #[display("atomic replace failed")]
        AtomicReplace(rustix::io::Errno),
        #[display("has no file name")]
        NoFileName,
        #[display("has no parent")]
        Parentless,
        #[display("failed to open parent directory")]
        OpenParent(io::Error),
        #[display("failed to create temporary file")]
        TempFileCreate(io::Error),
        #[display("failed to write to temporary file")]
        TempFileWrite(io::Error),
    }
}

pub fn atomic_overwrite(source_path: &Path, data: &[u8]) -> Result<(), Error> {
    atomic_overwrite_(source_path, data).map_err(|source| Error {
        source,
        path: source_path.to_owned()
    })
}

fn atomic_overwrite_(source_path: &Path, data: &[u8]) -> Result<(), ErrorKind> {
    let parent_path = source_path.parent()
        .map(|parent| if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        })
        .ok_or(ErrorKind::Parentless)?;

    let source_name = source_path.file_name()
        .ok_or(ErrorKind::NoFileName)?
        .to_owned();

    let temp_name = {
        let mut temp_name = source_name.to_owned();
        temp_name.push(".partial");
        temp_name
    };

    let temp_path = source_path.with_file_name(&temp_name);

    _ = std::fs::remove_file(&temp_path);

    let mut temp_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(ErrorKind::TempFileCreate)?;

    temp_file.write_all(data).map_err(ErrorKind::TempFileWrite)?;

    _ = temp_file.sync_all();

    let parent_dir = std::fs::File::open(parent_path)
        .map_err(ErrorKind::OpenParent)?;

    rustix::fs::renameat(&parent_dir, &temp_name, &parent_dir, &source_name)
        .map_err(ErrorKind::AtomicReplace)?;

    _ = parent_dir.sync_all();

    Ok(())
}
