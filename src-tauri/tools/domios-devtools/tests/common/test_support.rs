use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The harnesses resolve fixtures and sources against the `src-tauri` workspace root.
pub fn fixture_manifest_dir() -> &'static Path {
    static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();
    DIRECTORY.get_or_init(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(".."))
}
