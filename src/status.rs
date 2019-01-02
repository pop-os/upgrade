use std::io;
use std::process::ExitStatus;

pub trait StatusExt {
    fn as_result(self) -> io::Result<()>;
}

impl StatusExt for ExitStatus {
    fn as_result(self) -> io::Result<()> {
        if self.success() {
            Ok(())
        } else if let Some(127) = self.code() {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "command was not found",
            ))
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                format!("command failed with exit status: {}", self),
            ))
        }
    }
}
