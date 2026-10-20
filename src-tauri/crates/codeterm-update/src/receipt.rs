//! The installing client stages the target version; the next server start confirms it.

use std::path::{Path, PathBuf};

pub fn path() -> PathBuf {
    codeterm_paths::data_dir().join("analytics-update.json")
}

pub fn stage(path: &Path, target: &str) -> Result<(), String> {
    if target == codeterm_build_info::version() {
        return Ok(());
    }
    let staging = path.with_extension("pending");
    std::fs::write(&staging, target).map_err(|e| e.to_string())?;
    std::fs::rename(staging, path).map_err(|e| e.to_string())
}

pub fn stage_app(target: &str) {
    if stage(&path(), target).is_err() {
        log::debug!("[analytics] update receipt unavailable");
    }
}
