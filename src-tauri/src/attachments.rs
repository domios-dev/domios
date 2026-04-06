//! Shared attachment storage — temp file management for clipboard, drops, Telegram bridge.
//!
//! All transient files (pasted images, dropped files, downloaded Telegram attachments)
//! go through a single API. Each caller specifies a bucket (subdirectory) and optional
//! eviction policy. This consolidates what was duplicated across commands.rs, bridge/telegram.rs.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Default eviction: keep total bucket size under 100 MB.
pub const DEFAULT_MAX_BYTES: u64 = 100 * 1024 * 1024;

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

/// Apply eviction policy to a directory.
pub fn evict(dir: &Path, policy: EvictionPolicy) {
    let entries: Vec<_> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
        Err(_) => return,
    };

    let mut files: Vec<(PathBuf, u64, SystemTime)> = entries.iter()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            if !meta.is_file() { return None; }
            let modified = meta.modified().ok()?;
            Some((e.path(), meta.len(), modified))
        })
        .collect();

    // Sort oldest first — we delete from the front
    files.sort_by_key(|(_, _, t)| *t);

    match policy {
        EvictionPolicy::MaxBytes(limit) => {
            let total: u64 = files.iter().map(|(_, sz, _)| sz).sum();
            if total <= limit { return; }
            let mut remaining = total;
            for (path, sz, _) in &files {
                if remaining <= limit { break; }
                if std::fs::remove_file(path).is_ok() {
                    log::debug!("[attachments] evicted {} ({} bytes)", path.display(), sz);
                    remaining -= sz;
                }
            }
        }
        EvictionPolicy::MaxCount(limit) => {
            if files.len() <= limit { return; }
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
        let path = save(&bucket, "hello.txt", b"hello world", EvictionPolicy::MaxCount(10)).unwrap();
        assert!(path.exists());
        let content = std::fs::read(&path).unwrap();
        assert_eq!(content, b"hello world");
        // cleanup
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
        // Should not have ".." in extension
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
        // Create 5 files with delays so modified times differ
        for i in 0..5 {
            save(&bucket, &format!("f{}.txt", i), &[i as u8], EvictionPolicy::MaxCount(100)).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(15));
        }

        // Now trim to 3 — oldest 2 should go
        evict(&bucket_dir(&bucket), EvictionPolicy::MaxCount(3));

        let remaining: Vec<_> = std::fs::read_dir(bucket_dir(&bucket))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(remaining.len(), 3);
        // f0 and f1 should be gone (oldest), f2/f3/f4 remain
        assert!(!remaining.iter().any(|n| n == "f0.txt"));
        assert!(!remaining.iter().any(|n| n == "f1.txt"));
        assert!(remaining.iter().any(|n| n == "f4.txt"));

        let _ = std::fs::remove_dir_all(bucket_dir(&bucket));
    }

    #[test]
    fn test_max_bytes_eviction() {
        let bucket = unique_bucket();
        // Create 3 files of 1000 bytes each — exceeds 2500 byte limit
        for i in 0..3 {
            save(&bucket, &format!("f{}.bin", i), &vec![0u8; 1000], EvictionPolicy::MaxBytes(100_000)).unwrap();
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
        // Should not panic on empty dir
        evict(&dir, EvictionPolicy::MaxCount(10));
        evict(&dir, EvictionPolicy::MaxBytes(1000));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_evict_nonexistent_dir_no_panic() {
        let dir = std::env::temp_dir().join(format!("nonexistent-{}", uuid::Uuid::new_v4()));
        // Should not panic
        evict(&dir, EvictionPolicy::MaxCount(10));
    }
}
