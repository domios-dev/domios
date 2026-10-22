use std::io;
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
    crate::private_file::write_atomic(path, contents.as_ref())
}

#[cfg(test)]
mod tests;
