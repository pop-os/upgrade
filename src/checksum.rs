use async_fs::File;
use futures_util::AsyncReadExt;
use hex::FromHex;
use sha2::{digest::generic_array::GenericArray, Digest, Sha256};
use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ValidateError {
    #[error("checksum failed; expected {}, found {}", expected, found)]
    Checksum { expected: String, found: String },

    #[error("expected checksum isn't a valid checksum")]
    InvalidInput,

    #[error("I/O error while checksumming")]
    Io(#[from] io::Error),
}

pub async fn validate_checksum(file: &mut File, checksum: &str) -> Result<(), ValidateError> {
    info!("validating checksum of downloaded ISO");
    let expected = <[u8; 32]>::from_hex(checksum)
        .map(GenericArray::from)
        .map_err(|_| ValidateError::InvalidInput)?;

    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 8 * 1024];

    loop {
        match file.read(&mut buffer).await? {
            0 => break,
            read => hasher.update(&buffer[..read]),
        }
    }

    let found = hasher.finalize();
    if *found != *expected {
        return Err(ValidateError::Checksum {
            expected: checksum.into(),
            found:    format!("{:x}", found),
        });
    }

    Ok(())
}
