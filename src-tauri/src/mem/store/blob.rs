use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

pub(super) fn write_blob(blobs_dir: &Path, body: &str) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(body.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let filename = format!("sha256_{}.txt", hash);
    let path: PathBuf = blobs_dir.join(&filename);
    if !path.exists() {
        std::fs::write(&path, body).map_err(|e| format!("write blob: {}", e))?;
    }
    Ok(filename)
}

pub(super) fn read_blob(blobs_dir: &Path, filename: &str) -> Result<String, String> {
    let path = blobs_dir.join(filename);
    std::fs::read_to_string(&path).map_err(|e| format!("read blob {}: {}", filename, e))
}
