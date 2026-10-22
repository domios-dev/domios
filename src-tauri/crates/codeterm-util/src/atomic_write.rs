use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

// Per-writer unique tmp: a fixed name lets concurrent daemons clobber each other's tmp mid-write.
pub fn unique_tmp_path(path: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("sidecar");
    path.with_file_name(format!(".{name}.{pid}.{seq}.tmp"))
}

// Unique-temp write + fsync + atomic rename; best-effort cleanup of the temp on failure.
pub fn write_atomic(path: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    write_with_policy(path, contents.as_ref(), false)
}

pub(crate) fn write_private_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    write_with_policy(path, contents, true)
}

fn write_with_policy(path: &Path, contents: &[u8], private: bool) -> io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        if private {
            crate::private_file::create_dir_all(parent)?;
        } else {
            fs::create_dir_all(parent)?;
        }
    }
    let tmp = unique_tmp_path(path);
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        if private {
            crate::private_file::prepare_replacement(path, &tmp)?;
        } else {
            crate::private_file::preserve_permissions(path, &tmp)?;
        }
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests;
