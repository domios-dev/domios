use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

struct Capture {
    owner: String,
    root: Option<PathBuf>,
    paths: Mutex<Vec<PathBuf>>,
}

#[derive(Clone)]
pub struct CaptureContext(Arc<Capture>);

thread_local! {
    static CAPTURE: RefCell<Option<CaptureContext>> = const { RefCell::new(None) };
}

pub fn current_capture() -> Option<CaptureContext> {
    CAPTURE.with(|capture| capture.borrow().clone())
}

pub fn with_capture<T>(context: Option<CaptureContext>, operation: impl FnOnce() -> T) -> T {
    struct Restore(Option<CaptureContext>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CAPTURE.with(|capture| capture.replace(self.0.take()));
        }
    }
    let _restore = Restore(CAPTURE.with(|capture| capture.replace(context)));
    operation()
}

pub fn capture<T>(owner: &str, operation: impl FnOnce() -> T) -> (T, Vec<PathBuf>) {
    scoped(owner, None, operation)
}

/// Like `capture`, but files land under `root` instead of the instance task directory.
pub fn capture_in<T>(root: &Path, owner: &str, operation: impl FnOnce() -> T) -> (T, Vec<PathBuf>) {
    scoped(owner, Some(root.to_path_buf()), operation)
}

fn scoped<T>(
    owner: &str,
    root: Option<PathBuf>,
    operation: impl FnOnce() -> T,
) -> (T, Vec<PathBuf>) {
    let context = CaptureContext(Arc::new(Capture {
        owner: owner.to_string(),
        root,
        paths: Mutex::new(Vec::new()),
    }));
    let value = with_capture(Some(context.clone()), operation);
    let paths = context.0.paths.lock().clone();
    (value, paths)
}

pub fn write(contents: &str, suffix: Option<&str>) -> Option<String> {
    let (owner, root) = CAPTURE.with(|capture| {
        capture
            .borrow()
            .as_ref()
            .map(|capture| (capture.0.owner.clone(), capture.0.root.clone()))
            .unwrap_or_else(|| ("unscoped".to_string(), None))
    });
    let root = root.unwrap_or_else(task_dir);
    let marker = instance_marker_for_task_dir(&root);
    let path = write_at(&root, &marker, &owner, contents, suffix)?;
    Some(path.to_string_lossy().into_owned())
}

pub fn write_at(
    dir: &Path,
    marker: &str,
    owner: &str,
    contents: &str,
    suffix: Option<&str>,
) -> Option<PathBuf> {
    std::fs::create_dir_all(dir).ok()?;
    let suffix = safe_suffix(suffix.unwrap_or(""));
    let owner_hex = encode(owner.as_bytes());
    let unique = uuid::Uuid::new_v4().simple();
    let name = format!("codeterm-plugin-{marker}-{owner_hex}-{unique}{suffix}");
    let path = dir.join(name);
    std::fs::write(&path, contents).ok()?;
    CAPTURE.with(|capture| {
        if let Some(capture) = capture.borrow().as_ref() {
            capture.0.paths.lock().push(path.clone());
        }
    });
    Some(path)
}

pub fn task_dir() -> PathBuf {
    codeterm_paths::data_dir()
        .join("runtime")
        .join("task-files")
}

pub fn owned_paths(root: &Path, owner: &str) -> Vec<PathBuf> {
    let marker = instance_marker_for_task_dir(root);
    let prefix = file_prefix(&marker, owner);
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(&prefix))
        })
        .collect()
}

pub fn release_tab(root: &Path, owner: &str, paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut candidates = paths.to_vec();
    candidates.extend(owned_paths(root, owner));
    release_paths(root, owner, &candidates)
}

pub fn release_paths(root: &Path, owner: &str, paths: &[PathBuf]) -> Vec<PathBuf> {
    let marker = instance_marker_for_task_dir(root);
    let prefix = file_prefix(&marker, owner);
    let mut removed = Vec::new();
    for path in paths {
        if path.parent() != Some(root)
            || !path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(&prefix))
        {
            continue;
        }
        if std::fs::remove_file(path).is_ok() {
            removed.push(path.clone());
        }
    }
    removed
}

pub fn sweep(root: &Path, live_owners: &HashSet<String>) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let marker = instance_marker_for_task_dir(root);
    let own_prefix = format!("codeterm-plugin-{marker}-");
    let live: HashSet<_> = live_owners
        .iter()
        .map(|owner| encode(owner.as_bytes()))
        .collect();
    let mut removed = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(owner_hex) = name
            .strip_prefix(&own_prefix)
            .and_then(|rest| rest.split_once('-').map(|(owner, _)| owner))
        else {
            continue;
        };
        if !live.contains(owner_hex) && std::fs::remove_file(&path).is_ok() {
            removed.push(path);
        }
    }
    removed
}

fn file_prefix(marker: &str, owner: &str) -> String {
    format!("codeterm-plugin-{marker}-{}-", encode(owner.as_bytes()))
}

pub fn instance_marker_for(data_dir: &Path) -> String {
    data_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("codeterm")
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn instance_marker_for_task_dir(task_dir: &Path) -> String {
    task_dir
        .parent()
        .and_then(Path::parent)
        .map(instance_marker_for)
        .unwrap_or_else(|| "codeterm".into())
}

fn safe_suffix(suffix: &str) -> String {
    suffix
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
        .collect()
}

fn encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests;
