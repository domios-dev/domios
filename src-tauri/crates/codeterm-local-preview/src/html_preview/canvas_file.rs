use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use super::file::PreviewSource;
use crate::artifact_directive::{canvas_file_items, canvas_write_permitted, CanvasFileItem};

/// Canvas files share the 200,000-byte bound used by their chat representation.
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

type FileLocks = Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>;
static FILE_LOCKS: LazyLock<FileLocks> = LazyLock::new(Default::default);

/// Serializes this process's writers of one file; the entry goes when its last writer leaves.
struct FileLock {
    path: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl FileLock {
    fn new(path: &Path) -> Self {
        let mut locks = FILE_LOCKS.lock().unwrap_or_else(PoisonError::into_inner);
        let lock = locks.entry(path.to_path_buf()).or_default().clone();
        Self {
            path: path.to_path_buf(),
            lock,
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let mut locks = FILE_LOCKS.lock().unwrap_or_else(PoisonError::into_inner);
        if Arc::strong_count(&self.lock) == 2 {
            locks.remove(&self.path);
        }
    }
}

const REMOVED_PER_FILE: usize = 64;
const REMOVED_FILES: usize = 64;
type RemovedItems = Mutex<HashMap<PathBuf, VecDeque<CanvasFileItem>>>;
/// File items that owner writes removed from each file, so undo can put exactly them back.
/// Bounded per file and in files; a dropped record only ends that undo's ability to save.
static REMOVED: LazyLock<RemovedItems> = LazyLock::new(Default::default);

fn restorable(file: &Path) -> HashSet<CanvasFileItem> {
    REMOVED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(file)
        .map(|items| items.iter().cloned().collect())
        .unwrap_or_default()
}

fn remember_removed(file: &Path, before: &str, after: &str) {
    let (Some(before), Some(after)) = (canvas_file_items(before), canvas_file_items(after)) else {
        return;
    };
    let mut removed = REMOVED.lock().unwrap_or_else(PoisonError::into_inner);
    if !removed.contains_key(file) && removed.len() >= REMOVED_FILES {
        if let Some(evicted) = removed.keys().next().cloned() {
            removed.remove(&evicted);
        }
    }
    let items = removed.entry(file.to_path_buf()).or_default();
    for item in before.into_iter().filter(|item| !after.contains(item)) {
        if !items.contains(&item) {
            if items.len() >= REMOVED_PER_FILE {
                items.pop_front();
            }
            items.push_back(item);
        }
    }
}

/// Replaces the canvas atomically when it still holds `base`: write a sibling temp, then rename.
/// A file changed underneath returns its current text instead, so the edit lands on top of it.
///
/// Writers in this process take a per-file lock around read, compare and rename, so two edits
/// from one base never both succeed. External writers, such as an agent's editor, are not locked
/// out: the text is compared once more just before the rename, which leaves only the moment
/// between that comparison and the rename in which their write could be replaced.
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
    // Resolving the identity keeps no handle open, so it never blocks another writer's rename.
    let identity = super::checked_path(raw)?
        .canonicalize()
        .map_err(|error| format!("Unable to resolve preview file: {error}"))?;
    let file_lock = FileLock::new(&identity);
    let _held = file_lock
        .lock
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let (target, current) = read_canvas(raw, &guard)?;
    if current != base {
        return Ok(CanvasWrite::Conflict(current));
    }
    canvas_write_permitted(&current, next, &restorable(&identity)).map_err(String::from)?;
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
    if matches!(result, Ok(CanvasWrite::Written)) {
        remember_removed(&identity, &current, next);
    } else {
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
    // External writers hold no lock; this last comparison leaves them only the instant before rename.
    let (_, latest) = read_canvas(raw, guard)?;
    if latest != base {
        return Ok(CanvasWrite::Conflict(latest));
    }
    std::fs::rename(temp, target).map_err(|error| format!("Unable to replace canvas: {error}"))?;
    Ok(CanvasWrite::Written)
}

#[cfg(test)]
mod tests;
