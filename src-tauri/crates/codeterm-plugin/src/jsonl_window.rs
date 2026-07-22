//! Bounded, record-boundary-preserving reads over append-only JSONL session
//! files. Session files reach hundreds of MB in production, so every read must
//! be seek-based and capped — a whole-file read per poll starves the runtime.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Bytes one delta read pulls before yielding. Hitting the cap leaves the rest
/// for the next call, which is why [`JsonlChunk::more_available`] exists.
pub const DELTA_CHUNK_BYTES: u64 = 8 * 1024 * 1024;

/// A single record longer than this is abandoned instead of buffered; the
/// reader resyncs at the next boundary so the offset always advances.
pub const MAX_RECORD_BYTES: u64 = 64 * 1024 * 1024;

const SCAN_BUF_BYTES: usize = 64 * 1024;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct JsonlChunk {
    /// Whole records only — empty, or `\n`-terminated.
    pub bytes: Vec<u8>,
    /// Absolute offset one past the last consumed byte.
    pub new_offset: u64,
    /// The cap stopped the read before EOF; more complete records are already
    /// on disk and the caller may read again without waiting.
    pub more_available: bool,
}

impl JsonlChunk {
    fn held(offset: u64) -> Self {
        Self {
            bytes: Vec::new(),
            new_offset: offset,
            more_available: false,
        }
    }
}

/// Read whole JSONL records starting at `from_offset`, reading at most
/// `chunk_cap` bytes in the common case. A trailing partial line is held back
/// (offset unchanged) so a poll landing mid-write never splits a record.
pub fn read_records_from(
    path: &Path,
    from_offset: u64,
    chunk_cap: u64,
) -> std::io::Result<JsonlChunk> {
    let mut file = File::open(path)?;
    let total = file.metadata()?.len();
    if from_offset >= total {
        return Ok(JsonlChunk::held(from_offset));
    }
    file.seek(SeekFrom::Start(from_offset))?;

    let available = total - from_offset;
    let mut target = chunk_cap.max(1).min(available);
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let need = target.saturating_sub(buf.len() as u64);
        if need > 0 {
            (&mut file).take(need).read_to_end(&mut buf)?;
        }
        if let Some(idx) = buf.iter().rposition(|&b| b == b'\n') {
            buf.truncate(idx + 1);
            let new_offset = from_offset + buf.len() as u64;
            return Ok(JsonlChunk {
                bytes: buf,
                new_offset,
                more_available: new_offset < total,
            });
        }
        if from_offset + buf.len() as u64 >= total {
            return Ok(JsonlChunk::held(from_offset));
        }
        if buf.len() as u64 >= MAX_RECORD_BYTES {
            break;
        }
        target = target
            .saturating_mul(2)
            .min(available)
            .min(MAX_RECORD_BYTES);
    }

    let resume = skip_to_boundary(&mut file, from_offset + buf.len() as u64, total)?;
    log::warn!(
        "[jsonl_window] record at offset {} exceeds {} bytes — resyncing at {}",
        from_offset,
        MAX_RECORD_BYTES,
        resume
    );
    Ok(JsonlChunk {
        bytes: Vec::new(),
        new_offset: resume,
        more_available: resume < total,
    })
}

/// Read the byte range `[start, end)`, clamped to the file. Used by backfill,
/// which walks backwards from a known offset rather than tailing forwards.
pub fn read_range(path: &Path, start: u64, end: u64) -> std::io::Result<(Vec<u8>, u64)> {
    let mut file = File::open(path)?;
    let total = file.metadata()?.len();
    let end = end.min(total);
    let start = start.min(end);
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::with_capacity((end - start) as usize);
    (&mut file).take(end - start).read_to_end(&mut buf)?;
    Ok((buf, start))
}

/// First record boundary at or after `total - window`. Anchors a bounded tail
/// snapshot so the parser never sees a partial leading line. Returns `total`
/// when no boundary exists in the window (nothing complete to show).
pub fn tail_window_start(path: &Path, total: u64, window: u64) -> u64 {
    let rough = total.saturating_sub(window);
    if rough == 0 {
        return 0;
    }
    let Ok(mut file) = File::open(path) else {
        return total;
    };
    if file.seek(SeekFrom::Start(rough)).is_err() {
        return total;
    }
    let mut pos = rough;
    let mut buf = vec![0u8; SCAN_BUF_BYTES];
    loop {
        let Ok(n) = file.read(&mut buf) else {
            return total;
        };
        if n == 0 {
            return total;
        }
        if let Some(idx) = buf[..n].iter().position(|&b| b == b'\n') {
            return pos + idx as u64 + 1;
        }
        pos += n as u64;
    }
}

fn skip_to_boundary(file: &mut File, mut pos: u64, total: u64) -> std::io::Result<u64> {
    let mut buf = vec![0u8; SCAN_BUF_BYTES];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Ok(total);
        }
        if let Some(idx) = buf[..n].iter().position(|&b| b == b'\n') {
            return Ok(pos + idx as u64 + 1);
        }
        pos += n as u64;
    }
}

#[cfg(test)]
mod tests;
