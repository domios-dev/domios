use std::io::Read;

use super::protocol::{ExecOutputTooLarge, ExecResult};

pub(super) const LIMIT: usize = 128 * 1024;

#[derive(Default)]
pub(super) struct Capture {
    text: String,
    bytes: usize,
}

pub(super) fn capture(reader: &mut impl Read) -> Capture {
    let mut kept = Vec::new();
    let mut bytes = 0usize;
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                bytes = bytes.saturating_add(count);
                let retain = count.min(LIMIT.saturating_sub(kept.len()));
                kept.extend_from_slice(&chunk[..retain]);
            }
        }
    }
    Capture {
        text: String::from_utf8_lossy(&kept).into_owned(),
        bytes,
    }
}

pub(super) fn result(
    code: i32,
    stdout: Capture,
    stderr: Capture,
    error: Option<String>,
) -> ExecResult {
    let oversized = [("stdout", stdout.bytes), ("stderr", stderr.bytes)]
        .into_iter()
        .find(|(_, bytes)| *bytes > LIMIT)
        .map(|(stream, actual_bytes)| ExecOutputTooLarge {
            stream: stream.into(),
            limit_bytes: LIMIT,
            actual_bytes,
        });
    ExecResult {
        code: Some(code),
        stdout: Some(stdout.text),
        stderr: Some(stderr.text),
        error: error.or_else(|| oversized.as_ref().map(|size| format!(
            "output_too_large: {} produced {} bytes; limit is {} bytes; project or page subprocess output before returning it",
            size.stream, size.actual_bytes, size.limit_bytes
        ))),
        truncated: oversized.as_ref().map(|_| true),
        output_too_large: oversized,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests;
