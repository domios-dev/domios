pub use crate::backend::sqlite::fixtures::seed_schema;
#[cfg(all(test, feature = "embeddings"))]
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

#[cfg(test)]
pub(crate) fn offline_directory() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    seed_schema(directory.path());
    directory
}

#[cfg(all(test, feature = "embeddings"))]
pub(crate) fn fixture_manifest_dir() -> &'static Path {
    static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();
    DIRECTORY.get_or_init(|| {
        std::env::var_os("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
    })
}

#[cfg(all(test, feature = "test-support"))]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub fn archive_mem_item_conn(
    conn: &rusqlite::Connection,
    id: &str,
    reason: &str,
    actor: &str,
) -> Result<serde_json::Value, (u16, String)> {
    crate::backend::sqlite::archive_item(
        &crate::MemManager::write_context(),
        conn,
        id,
        reason,
        actor,
    )
    .map_err(|e| (e.status_code(), e.into_message()))
}

#[cfg(all(test, feature = "test-support"))]
use crate::backend::sqlite::fixtures::memory_connection;
