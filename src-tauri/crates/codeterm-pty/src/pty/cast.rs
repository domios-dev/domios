//! Asciinema v2 cast parsing and OutputBuffer injection for dev e2e replay.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::capture::decode_output_event;
use super::manager::OutputBuffer;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CastEvent {
    Output { at_ms: u64, bytes: Vec<u8> },
    Resize { at_ms: u64, cols: u16, rows: u16 },
}

#[derive(Clone, Debug)]
pub struct ParsedCast {
    pub width: u16,
    pub height: u16,
    pub events: Vec<CastEvent>,
}

#[derive(Deserialize)]
struct CastHeader {
    width: Option<u64>,
    height: Option<u64>,
}

pub fn repo_root() -> PathBuf {
    #[cfg(test)]
    let root = crate::test_support::fixture_manifest_dir().join("../../..");
    #[cfg(not(test))]
    let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.."));
    root
}

/// Resolve a cast path to an absolute file under `testbench/replays/`.
pub fn resolve_replay_path(repo_root: &Path, raw: &str) -> Result<PathBuf, String> {
    let replays = repo_root.join("testbench/replays");
    let candidate = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else if raw.starts_with("testbench/") {
        repo_root.join(raw)
    } else {
        replays.join(raw)
    };
    if !candidate.exists() {
        return Err(format!("cast file not found: {}", candidate.display()));
    }
    let canon = candidate
        .canonicalize()
        .map_err(|e| format!("cast path: {e}"))?;
    let replays_canon = replays.canonicalize().unwrap_or_else(|_| replays.clone());
    if !canon.starts_with(&replays_canon) {
        return Err("cast_path must resolve under testbench/replays".into());
    }
    Ok(canon)
}

pub fn parse_cast_text(text: &str) -> Result<ParsedCast, String> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header_line = lines.next().ok_or("cast: missing header line")?;
    let header: CastHeader =
        serde_json::from_str(header_line).map_err(|e| format!("cast header: {e}"))?;
    let width = header.width.unwrap_or(80) as u16;
    let height = header.height.unwrap_or(24) as u16;
    let mut events = Vec::new();
    for (i, line) in lines.enumerate() {
        let row: serde_json::Value =
            serde_json::from_str(line).map_err(|e| format!("cast row {}: {e}", i + 2))?;
        let arr = row
            .as_array()
            .ok_or_else(|| format!("cast row {}: expected array", i + 2))?;
        if arr.len() < 3 {
            return Err(format!("cast row {}: expected [t, code, data]", i + 2));
        }
        let at_ms = (arr[0]
            .as_f64()
            .ok_or_else(|| format!("cast row {}: time", i + 2))?
            * 1000.0)
            .round() as u64;
        let code = arr[1]
            .as_str()
            .ok_or_else(|| format!("cast row {}: code", i + 2))?;
        match code {
            "o" | "o+b64" => {
                let data = arr[2]
                    .as_str()
                    .ok_or_else(|| format!("cast row {}: data", i + 2))?;
                let bytes = decode_output_event(code, data)
                    .map_err(|e| format!("cast row {}: {e}", i + 2))?
                    .ok_or_else(|| format!("cast row {}: empty output", i + 2))?;
                events.push(CastEvent::Output { at_ms, bytes });
            }
            "r" => {
                let data = arr[2]
                    .as_str()
                    .ok_or_else(|| format!("cast row {}: data", i + 2))?;
                let (cols, rows) = data
                    .split_once('x')
                    .ok_or_else(|| format!("cast row {}: resize COLSxROWS", i + 2))?;
                events.push(CastEvent::Resize {
                    at_ms,
                    cols: cols
                        .parse()
                        .map_err(|_| format!("cast row {}: resize cols", i + 2))?,
                    rows: rows
                        .parse()
                        .map_err(|_| format!("cast row {}: resize rows", i + 2))?,
                });
            }
            "i" => {}
            other => return Err(format!("cast row {}: unsupported event {other}", i + 2)),
        }
    }
    Ok(ParsedCast {
        width,
        height,
        events,
    })
}

pub fn parse_cast_file(path: &Path) -> Result<ParsedCast, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    parse_cast_text(&text)
}

/// Append output events into `OutputBuffer`. Returns (output_bytes, final offset).
pub fn inject_output_events(
    buf: &mut OutputBuffer,
    pty_id: &str,
    events: &[CastEvent],
) -> (usize, u64) {
    let mut nbytes = 0usize;
    for ev in events {
        if let CastEvent::Output { bytes, .. } = ev {
            if bytes.is_empty() {
                continue;
            }
            buf.append(pty_id, bytes);
            nbytes += bytes.len();
        }
    }
    (nbytes, buf.total_written(pty_id))
}

pub fn output_byte_count(events: &[CastEvent]) -> usize {
    events
        .iter()
        .filter_map(|ev| match ev {
            CastEvent::Output { bytes, .. } => Some(bytes.len()),
            _ => None,
        })
        .sum()
}

#[cfg(test)]
mod tests;
