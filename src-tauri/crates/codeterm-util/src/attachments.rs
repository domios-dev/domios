//! Shared attachment storage — temp file management for clipboard, drops, Telegram bridge.
//!
//! All transient files (pasted images, dropped files, downloaded Telegram attachments)
//! go through a single API. Each caller specifies a bucket (subdirectory) and optional
//! eviction policy. This consolidates what was duplicated across commands.rs, bridge/telegram.rs.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Default eviction: keep total bucket size under 100 MB.
pub const DEFAULT_MAX_BYTES: u64 = 100 * 1024 * 1024;

/// Inbound chat files land here as short safe ASCII paths: `codeterm-chat/<id>.<ext>`.
const CHAT_BUCKET: &str = "chat";

/// Below this size we byte-copy (cheap, and decouples from the source's lifetime).
/// At or above it we prefer a symlink/hardlink so large files aren't duplicated into temp.
pub const SAFE_LINK_THRESHOLD: u64 = 4 * 1024 * 1024;

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
    let dir = bucket_dir(bucket);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create bucket dir: {}", e))?;
    let path = dir.join(filename);
    std::fs::write(&path, bytes).map_err(|e| format!("write file: {}", e))?;
    evict(&dir, policy);
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

/// Normalize an inbound native chat file to a short safe ASCII path under the
/// chat temp dir (`codeterm-chat/<id>.<ext>`) so downstream tooling and the
/// agent's file-read can always open it. Unicode/space/special-char basenames
/// (e.g. a macOS screenshot `Снимок экрана — 2026-... .png`) otherwise break.
///
/// Pass-through (no alias) when the path is already safe ASCII and stable, or
/// already inside the chat bucket (idempotent). Large stable files are linked,
/// not copied, to avoid duplicating bytes into temp; ephemeral/small files copy.
/// How a dropped path reaches the composer: a folder stays a path, anything readable is a file.
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
    if path.starts_with(&dir) {
        return Ok(path.to_path_buf());
    }
    let basename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let ephemeral = is_volatile_source(path);
    if is_safe_ascii_basename(&basename) && !ephemeral {
        return Ok(path.to_path_buf());
    }

    std::fs::create_dir_all(&dir).map_err(|e| format!("create chat dir: {}", e))?;
    sweep_dangling_symlinks(&dir);

    let id = short_id(path);
    let target = match lower_ext(path) {
        Some(ext) => dir.join(format!("{}.{}", id, ext)),
        None => dir.join(id),
    };
    // Idempotent reuse: a valid alias/copy for this source already exists.
    if std::fs::metadata(&target).is_ok() {
        return Ok(target);
    }
    let _ = std::fs::remove_file(&target);

    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if ephemeral || size < SAFE_LINK_THRESHOLD {
        std::fs::copy(path, &target).map_err(|e| format!("copy attachment: {}", e))?;
    } else {
        link_or_copy(path, &target)?;
    }
    evict(&dir, EvictionPolicy::MaxBytes(DEFAULT_MAX_BYTES));
    Ok(target)
}

fn is_safe_ascii_basename(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_'))
}

/// Short stable id derived from the absolute source path — same source reuses
/// the same alias slot, so repeated drops of one file don't pile up.
fn short_id(source: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    source.to_string_lossy().hash(&mut h);
    format!("{:08x}", h.finish() as u32)
}

fn lower_ext(source: &Path) -> Option<String> {
    source
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .filter(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// Symlink first (zero data copy), hardlink if same volume, copy as last resort.
fn link_or_copy(source: &Path, target: &Path) -> Result<(), String> {
    if symlink_file(source, target).is_ok() {
        return Ok(());
    }
    if std::fs::hard_link(source, target).is_ok() {
        return Ok(());
    }
    std::fs::copy(source, target)
        .map(|_| ())
        .map_err(|e| format!("copy attachment: {}", e))
}

#[cfg(unix)]
fn symlink_file(source: &Path, target: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, target)
}

#[cfg(windows)]
fn symlink_file(source: &Path, target: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(source, target)
}

#[cfg(not(any(unix, windows)))]
fn symlink_file(_source: &Path, _target: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "symlink unsupported",
    ))
}

/// A symlink whose target vanished (`metadata` follows the link and fails) is
/// dead weight — drop it so a fresh drop re-materializes cleanly.
fn sweep_dangling_symlinks(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let is_symlink = entry.file_type().map(|t| t.is_symlink()).unwrap_or(false);
        if is_symlink && std::fs::metadata(entry.path()).is_err() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn is_volatile_source(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str().to_string_lossy() == "TemporaryItems")
        && path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("NSIRD_screencaptureui_"))
}

/// Apply eviction policy to a directory.
pub fn evict(dir: &Path, policy: EvictionPolicy) {
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
                if std::fs::remove_file(path).is_ok() {
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
            for (path, _, _) in files.iter().take(drop_count) {
                if std::fs::remove_file(path).is_ok() {
                    log::debug!("[attachments] evicted {}", path.display());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_bucket() -> String {
        format!("test-{}", uuid::Uuid::new_v4())
    }

    #[test]
    fn test_save_and_read_back() {
        let bucket = unique_bucket();
        let path = save(
            &bucket,
            "hello.txt",
            b"hello world",
            EvictionPolicy::MaxCount(10),
        )
        .unwrap();
        assert!(path.exists());
        let content = std::fs::read(&path).unwrap();
        assert_eq!(content, b"hello world");
        let _ = std::fs::remove_dir_all(bucket_dir(&bucket));
    }

    #[test]
    fn test_save_with_uuid_generates_unique_names() {
        let bucket = unique_bucket();
        let path1 = save_with_uuid(&bucket, "png", b"one", EvictionPolicy::MaxCount(10)).unwrap();
        let path2 = save_with_uuid(&bucket, "png", b"two", EvictionPolicy::MaxCount(10)).unwrap();
        assert_ne!(path1, path2);
        assert!(path1.exists());
        assert!(path2.exists());
        let _ = std::fs::remove_dir_all(bucket_dir(&bucket));
    }

    #[test]
    fn test_save_with_uuid_handles_leading_dot() {
        let bucket = unique_bucket();
        let path = save_with_uuid(&bucket, ".png", b"data", EvictionPolicy::MaxCount(10)).unwrap();
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(!name.contains(".."));
        assert!(name.ends_with(".png"));
        let _ = std::fs::remove_dir_all(bucket_dir(&bucket));
    }

    #[test]
    fn test_save_without_extension() {
        let bucket = unique_bucket();
        let path = save_with_uuid(&bucket, "", b"data", EvictionPolicy::MaxCount(10)).unwrap();
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(!name.contains('.'));
        let _ = std::fs::remove_dir_all(bucket_dir(&bucket));
    }

    #[test]
    fn test_max_count_eviction_keeps_newest() {
        let bucket = unique_bucket();
        for i in 0..5 {
            save(
                &bucket,
                &format!("f{}.txt", i),
                &[i as u8],
                EvictionPolicy::MaxCount(100),
            )
            .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(15));
        }

        evict(&bucket_dir(&bucket), EvictionPolicy::MaxCount(3));

        let remaining: Vec<_> = std::fs::read_dir(bucket_dir(&bucket))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(remaining.len(), 3);
        assert!(!remaining.iter().any(|n| n == "f0.txt"));
        assert!(!remaining.iter().any(|n| n == "f1.txt"));
        assert!(remaining.iter().any(|n| n == "f4.txt"));

        let _ = std::fs::remove_dir_all(bucket_dir(&bucket));
    }

    #[test]
    fn test_max_bytes_eviction() {
        let bucket = unique_bucket();
        for i in 0..3 {
            save(
                &bucket,
                &format!("f{}.bin", i),
                &vec![0u8; 1000],
                EvictionPolicy::MaxBytes(100_000),
            )
            .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(15));
        }

        evict(&bucket_dir(&bucket), EvictionPolicy::MaxBytes(2500));

        let remaining: Vec<_> = std::fs::read_dir(bucket_dir(&bucket))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        // Should have 2 files remaining (2000 bytes ≤ 2500)
        assert_eq!(remaining.len(), 2);

        let _ = std::fs::remove_dir_all(bucket_dir(&bucket));
    }

    #[test]
    fn test_evict_empty_dir_no_panic() {
        let bucket = unique_bucket();
        let dir = bucket_dir(&bucket);
        std::fs::create_dir_all(&dir).unwrap();
        evict(&dir, EvictionPolicy::MaxCount(10));
        evict(&dir, EvictionPolicy::MaxBytes(1000));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_evict_nonexistent_dir_no_panic() {
        let dir = std::env::temp_dir().join(format!("nonexistent-{}", uuid::Uuid::new_v4()));
        evict(&dir, EvictionPolicy::MaxCount(10));
    }

    #[test]
    fn test_materialize_dropped_temp_screenshot_to_durable_path() {
        let temp_root =
            std::env::temp_dir().join(format!("codeterm-source-{}", uuid::Uuid::new_v4()));
        let temp_dir = temp_root.join("TemporaryItems");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let source = temp_dir.join("NSIRD_screencaptureui_abc123.png");
        std::fs::write(&source, b"screenshot").unwrap();
        let materialized = materialize_dropped_path(&source).unwrap();

        assert!(materialized.exists());
        assert_ne!(materialized, source);
        // Ephemeral source is copied, not linked, so it survives the original being reaped.
        assert_eq!(std::fs::read(&materialized).unwrap(), b"screenshot");
        assert!(materialized.starts_with(bucket_dir(CHAT_BUCKET)));

        let _ = std::fs::remove_file(&materialized);
        let _ = std::fs::remove_dir_all(temp_root);
    }

    #[test]
    fn test_non_temp_safe_ascii_path_is_referenced_raw() {
        let source = std::env::temp_dir().join(format!("normal-drop-{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&source, b"normal").unwrap();
        let referenced = materialize_dropped_path(&source).unwrap();
        assert_eq!(referenced, source);
        let _ = std::fs::remove_file(source);
    }

    #[test]
    fn test_unsafe_unicode_path_maps_to_safe_chat_path() {
        let dir = std::env::temp_dir().join(format!("codeterm-src-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("Снимок экрана — 2026-06-23 в 14.07.27.png");
        std::fs::write(&source, b"img").unwrap();

        let safe = materialize_dropped_path(&source).unwrap();
        let name = safe.file_name().unwrap().to_string_lossy().to_string();

        assert!(safe.starts_with(bucket_dir(CHAT_BUCKET)));
        assert!(
            is_safe_ascii_basename(&name),
            "basename not safe ASCII: {name}"
        );
        // Extension preserved (lowercased) so type detection + image read still work.
        assert!(name.ends_with(".png"), "ext not preserved: {name}");
        assert!(std::fs::metadata(&safe).is_ok(), "safe path unreadable");

        let _ = std::fs::remove_file(&safe);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_extension_lowercased_and_preserved() {
        let dir = std::env::temp_dir().join(format!("codeterm-src-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("файл копия.JPG");
        std::fs::write(&source, b"x").unwrap();

        let safe = materialize_dropped_path(&source).unwrap();
        let name = safe.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.ends_with(".jpg"), "ext should be lowercased: {name}");

        let _ = std::fs::remove_file(&safe);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_idempotent_on_already_safe_chat_path() {
        let dir = std::env::temp_dir().join(format!("codeterm-src-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("отчёт.pdf");
        std::fs::write(&source, b"pdf").unwrap();

        let safe = materialize_dropped_path(&source).unwrap();
        // Re-feeding an already-safe chat-bucket path returns it unchanged (no double-copy).
        let again = materialize_dropped_path(&safe).unwrap();
        assert_eq!(safe, again);

        let _ = std::fs::remove_file(&safe);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_large_stable_file_is_linked_not_copied() {
        let dir = std::env::temp_dir().join(format!("codeterm-src-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        // Unsafe name + above the link threshold -> aliased without byte duplication.
        let source = dir.join("большой файл.bin");
        std::fs::write(&source, vec![7u8; (SAFE_LINK_THRESHOLD + 1024) as usize]).unwrap();

        let safe = materialize_dropped_path(&source).unwrap();
        assert!(safe.starts_with(bucket_dir(CHAT_BUCKET)));
        assert_eq!(std::fs::read(&safe).unwrap().len(), source_len(&source));

        let _ = std::fs::remove_file(&safe);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_dropped_folder_keeps_its_path_and_kind() {
        let dir = std::env::temp_dir().join(format!("codeterm-папка-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("notes.txt");
        std::fs::write(&file, b"x").unwrap();

        assert_eq!(dropped_path_kind(&dir), "folder");
        assert_eq!(dropped_path_kind(&file), "file");
        assert_eq!(dropped_path_kind(&dir.join("absent")), "missing");
        assert_eq!(materialize_dropped_path(&dir).unwrap(), dir);

        let _ = std::fs::remove_dir_all(dir);
    }

    fn source_len(p: &Path) -> usize {
        std::fs::metadata(p).unwrap().len() as usize
    }
}
