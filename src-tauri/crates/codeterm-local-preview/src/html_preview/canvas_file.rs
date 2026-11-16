use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::file::PreviewSource;
use crate::artifact_directive::canvas_write_permitted;

/// A canvas file holds the same JSON as an inline `<ds-canvas>` body, so it shares its bound.
pub const CANVAS_FILE_LIMIT: u64 = 200_000;

#[derive(Debug, PartialEq, Eq)]
pub enum CanvasWrite {
    Written,
    /// The file changed since the editor read it; the editor reapplies its edit on this text.
    Conflict(String),
}

pub fn is_canvas_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.len() > 12 && name.to_ascii_lowercase().ends_with(".canvas.json"))
}

type Guard<'a> = dyn Fn(&Path, Option<&Path>) -> Result<(), String> + 'a;

/// Reads the canvas through the preview guard; the handle closes before the caller replaces it.
fn read_canvas(raw: &str, guard: &Guard<'_>) -> Result<(PathBuf, String), String> {
    let mut source = PreviewSource::open(raw, &|path, canonical| guard(path, canonical))?;
    if !is_canvas_file(&source.path) {
        return Err("Canvas preview source must use .canvas.json".into());
    }
    let bytes = source.read(CANVAS_FILE_LIMIT)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| "Binary or non-UTF-8 file: text preview is unavailable".to_string())?;
    Ok((source.path.clone(), text))
}

/// Succeeds when the guard admits this canvas file for writing.
pub fn probe_canvas_file(
    raw: &str,
    guard: impl Fn(&Path, Option<&Path>) -> Result<(), String>,
) -> Result<(), String> {
    read_canvas(raw, &guard).map(|_| ())
}

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Replaces the canvas atomically when it still holds `base`: write a sibling temp, then rename.
/// A file changed underneath returns its current text instead, so the edit lands on top of it.
pub fn write_canvas_file(
    raw: &str,
    guard: impl Fn(&Path, Option<&Path>) -> Result<(), String>,
    base: &str,
    next: &str,
) -> Result<CanvasWrite, String> {
    if next.len() as u64 > CANVAS_FILE_LIMIT {
        return Err(format!(
            "Local preview source exceeds {CANVAS_FILE_LIMIT} bytes"
        ));
    }
    let (target, current) = read_canvas(raw, &guard)?;
    if current != base {
        return Ok(CanvasWrite::Conflict(current));
    }
    canvas_write_permitted(&current, next).map_err(String::from)?;
    let parent = target.parent().ok_or("Canvas file has no directory")?;
    let name = target
        .file_name()
        .ok_or("Canvas file has no name")?
        .to_string_lossy();
    let temp = parent.join(format!(
        ".{name}.{}-{}.tmp",
        std::process::id(),
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = replace(raw, &guard, base, next, &temp, &target);
    if !matches!(result, Ok(CanvasWrite::Written)) {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn replace(
    raw: &str,
    guard: &Guard<'_>,
    base: &str,
    next: &str,
    temp: &Path,
    target: &Path,
) -> Result<CanvasWrite, String> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp)
        .map_err(|error| format!("Unable to write canvas: {error}"))?;
    file.write_all(next.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("Unable to write canvas: {error}"))?;
    drop(file);
    // The second read narrows the window in which an agent's write could be replaced unseen.
    let (_, latest) = read_canvas(raw, guard)?;
    if latest != base {
        return Ok(CanvasWrite::Conflict(latest));
    }
    std::fs::rename(temp, target).map_err(|error| format!("Unable to replace canvas: {error}"))?;
    Ok(CanvasWrite::Written)
}

#[cfg(test)]
mod tests;
