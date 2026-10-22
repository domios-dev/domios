use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Default eviction: keep total bucket size under 100 MB.
pub const DEFAULT_MAX_BYTES: u64 = 100 * 1024 * 1024;
pub const CHAT_UPLOAD_CACHE_BYTES: u64 = 1024 * 1024 * 1024;

/// Inbound chat files land here as short safe ASCII paths: `codeterm-chat/<id>.<ext>`.
const CHAT_BUCKET: &str = "chat";

/// Eviction policy for a bucket.
#[derive(Debug, Clone, Copy)]
pub enum EvictionPolicy {
    /// Keep total size under this many bytes (delete oldest first).
    MaxBytes(u64),
    /// Keep this many most-recent files (delete oldest first).
    MaxCount(usize),
}

/// Root directory for a bucket: `%TEMP%/codeterm-{bucket}/`.
pub fn bucket_dir(bucket: &str) -> PathBuf {
    std::env::temp_dir().join(format!("codeterm-{}", bucket))
}

/// Save bytes to a file in the given bucket. Returns the absolute path.
///
/// - `bucket`: subdirectory name (e.g. "images", "drops", "telegram")
/// - `filename`: target filename (caller should ensure uniqueness if needed)
/// - `bytes`: file content
/// - `policy`: eviction policy applied after write
pub fn save(
    bucket: &str,
    filename: &str,
    bytes: &[u8],
    policy: EvictionPolicy,
) -> Result<PathBuf, String> {
    if bytes.is_empty() {
        return Err("Empty file".into());
    }
    let dir = bucket_dir(bucket);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create bucket dir: {}", e))?;
    let path = dir.join(filename);
    std::fs::write(&path, bytes).map_err(|e| format!("write file: {}", e))?;
    evict_preserving(&dir, policy, Some(&path));
    Ok(path)
}

/// Save bytes to a file with an auto-generated unique name using the given extension.
/// Useful for clipboard pastes where the original filename is unknown.
pub fn save_with_uuid(
    bucket: &str,
    extension: &str,
    bytes: &[u8],
    policy: EvictionPolicy,
) -> Result<PathBuf, String> {
    let ext = extension.trim_start_matches('.');
    let filename = if ext.is_empty() {
        format!("paste-{}", uuid::Uuid::new_v4())
    } else {
        format!("paste-{}.{}", uuid::Uuid::new_v4(), ext)
    };
    save(bucket, &filename, bytes, policy)
}

/// Save bytes preserving a user-provided filename, prefixed with a short UUID for uniqueness.
/// Falls back to `paste-<uuid>` when `name` is empty.
pub fn save_with_prefixed_name(
    bucket: &str,
    name: &str,
    bytes: &[u8],
    policy: EvictionPolicy,
) -> Result<PathBuf, String> {
    let prefix = &uuid::Uuid::new_v4().to_string()[..8];
    let filename = if name.is_empty() {
        format!("paste-{}", prefix)
    } else {
        format!("{}-{}", prefix, name)
    };
    save(bucket, &filename, bytes, policy)
}

mod source;
pub use source::{copy_complete_file, read_complete_file};

pub fn dropped_path_kind(path: &Path) -> &'static str {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => "folder",
        Ok(_) => "file",
        Err(_) => "missing",
    }
}

pub fn materialize_dropped_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_dir() {
        return Ok(path.to_path_buf());
    }
    let dir = bucket_dir(CHAT_BUCKET);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create chat dir: {e}"))?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let target = match lower_ext(path) {
        Some(ext) => dir.join(format!("{id}.{ext}")),
        None => dir.join(id),
    };
    copy_complete_file(path, &target, CHAT_UPLOAD_CACHE_BYTES)?;
    evict_preserving(
        &dir,
        EvictionPolicy::MaxBytes(CHAT_UPLOAD_CACHE_BYTES),
        Some(&target),
    );
    Ok(target)
}

fn lower_ext(source: &Path) -> Option<String> {
    source
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .filter(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// Apply eviction policy to a directory.
pub fn evict(dir: &Path, policy: EvictionPolicy) {
    evict_preserving(dir, policy, None);
}

fn evict_preserving(dir: &Path, policy: EvictionPolicy, retained: Option<&Path>) {
    let entries: Vec<_> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
        Err(_) => return,
    };

    let mut files: Vec<(PathBuf, u64, SystemTime)> = entries
        .iter()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            let modified = meta.modified().ok()?;
            Some((e.path(), meta.len(), modified))
        })
        .collect();

    files.sort_by_key(|(_, _, t)| *t);

    match policy {
        EvictionPolicy::MaxBytes(limit) => {
            let total: u64 = files.iter().map(|(_, sz, _)| sz).sum();
            if total <= limit {
                return;
            }
            let mut remaining = total;
            for (path, sz, _) in &files {
                if remaining <= limit {
                    break;
                }
                if retained != Some(path.as_path()) && std::fs::remove_file(path).is_ok() {
                    log::debug!("[attachments] evicted {} ({} bytes)", path.display(), sz);
                    remaining -= sz;
                }
            }
        }
        EvictionPolicy::MaxCount(limit) => {
            if files.len() <= limit {
                return;
            }
            let drop_count = files.len() - limit;
            for (path, _, _) in files
                .iter()
                .filter(|(path, _, _)| retained != Some(path.as_path()))
                .take(drop_count)
            {
                if retained != Some(path.as_path()) && std::fs::remove_file(path).is_ok() {
                    log::debug!("[attachments] evicted {}", path.display());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
