use err_derive::Error;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read},
};

#[derive(Debug, Error)]
pub enum ValidateError {
    #[error(display = "checksum failed; expected {}, found {}", expected, found)]
    Checksum { expected: String, found: String },
    #[error(display = "I/O error while checksumming: {}", _0)]
    Io(io::Error),
}

pub fn validate_checksum(file: &mut File, checksum: &str) -> Result<(), ValidateError> {
    eprintln!("validating checksum of downloaded ISO");
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];

    loop {
        let read = file.read(&mut buffer).map_err(ValidateError::Io)?;
        if read == 0 {
            break;
        }
        hasher.input(&buffer[..read]);
    }

    let found = format!("{:x}", hasher.result());
    if found != checksum {
        return Err(ValidateError::Checksum { expected: checksum.into(), found });
    }

    Ok(())
}
