use std::fs::File;
use std::io::{self, Seek, Write};

use codeterm_blueprint_types::blueprint_execution::ExecutionEnvelope;

fn encode(previous: &str) -> io::Result<Vec<u8>> {
    ExecutionEnvelope::from_text(previous)
        .encode()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "contract_violation"))
}

pub(super) fn file(previous: &str) -> io::Result<File> {
    file_bytes(&encode(previous)?)
}

pub(super) fn file_bytes(bytes: &[u8]) -> io::Result<File> {
    if bytes.len() > 32768 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "contract_violation",
        ));
    }
    let mut file = tempfile::tempfile()?;
    file.write_all(bytes)?;
    file.rewind()?;
    Ok(file)
}

#[cfg(test)]
mod tests;
