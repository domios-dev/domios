use std::fs::File;
use std::io::{self, Seek, Write};

use serde_json::{json, Value};

fn encode(previous: &str) -> Vec<u8> {
    let limit = super::capture::PREFIX_BYTES;
    if previous.len() <= limit {
        let value = serde_json::from_str::<Value>(previous)
            .unwrap_or_else(|_| Value::String(previous.into()));
        let bytes = value.to_string().into_bytes();
        if bytes.len() <= limit {
            return bytes;
        }
    }
    json!({"type":"truncated", "truncated":true, "total_bytes":previous.len(),
        "limit_bytes":limit, "prefix":previous.chars().take((limit - 256) / 6).collect::<String>()})
    .to_string()
    .into_bytes()
}

pub(super) fn file(previous: &str) -> io::Result<File> {
    let mut file = tempfile::tempfile()?;
    file.write_all(&encode(previous))?;
    file.rewind()?;
    Ok(file)
}

#[cfg(test)]
mod tests;
