use super::*;
use std::path::Path;
impl SqliteMemStore {
    pub fn open_read_only(db_path: &Path, blobs_dir: &Path) -> Result<Self, ReadOnlyOpenError> {
        crate::embeddings::ensure_extension_registered();
        let conn = Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| ReadOnlyOpenError::Storage(error.to_string()))?;
        let version: Result<i64, _> =
            conn.query_row("SELECT version FROM mem_schema_version", [], |row| {
                row.get(0)
            });
        if version.ok() != Some(codeterm_mem_store::migrations::SCHEMA_VERSION) {
            return Err(ReadOnlyOpenError::SourceSchemaMismatch);
        }
        conn.execute_batch("PRAGMA query_only=ON; PRAGMA foreign_keys=ON;")
            .map_err(|error| ReadOnlyOpenError::Storage(error.to_string()))?;
        Ok(Self::from_connection(
            Arc::new(Mutex::new(conn)),
            blobs_dir.to_path_buf(),
        ))
    }
    pub fn open(db_path: &Path, blobs_dir: &Path) -> Result<Self, String> {
        Self::open_writable(db_path, blobs_dir, true)
    }
    pub fn open_for_import(db_path: &Path, blobs_dir: &Path) -> Result<Self, String> {
        Self::open_writable(db_path, blobs_dir, false)
    }
    fn open_writable(db_path: &Path, blobs_dir: &Path, initialize: bool) -> Result<Self, String> {
        crate::embeddings::ensure_extension_registered();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::create_dir_all(blobs_dir).map_err(|e| e.to_string())?;
        let conn = Connection::open(db_path).map_err(|e| format!("open knowledge.db: {e}"))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        store::init_schema(&conn).map_err(|e| format!("init schema: {e}"))?;
        if initialize {
            crate::backend::sqlite::blueprint::cleanup::remove_v1(&conn, blobs_dir)?;
            crate::backend::sqlite::blueprint::metadata::initialize(&conn)?;
        } else {
            crate::backend::sqlite::blueprint::initialize(&conn)?;
        }
        Ok(Self::from_connection(
            Arc::new(Mutex::new(conn)),
            blobs_dir.to_path_buf(),
        ))
    }
    #[cfg(feature = "test-support")]
    pub(crate) fn open_fixture_memory(blobs_dir: &Path) -> Result<Self, String> {
        let conn = super::fixtures::memory_connection()?;
        std::fs::create_dir_all(blobs_dir).map_err(|e| e.to_string())?;
        Ok(Self::from_connection(
            Arc::new(Mutex::new(conn)),
            blobs_dir.to_path_buf(),
        ))
    }
}

#[derive(Debug)]
pub enum ReadOnlyOpenError {
    SourceSchemaMismatch,
    Storage(String),
}

impl std::fmt::Display for ReadOnlyOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SourceSchemaMismatch => f.write_str(
                "memory_source_schema_mismatch: expected current schema version 2; older schemas cannot be upgraded",
            ),
            Self::Storage(error) => write!(f, "memory_source_open: {error}"),
        }
    }
}

impl std::error::Error for ReadOnlyOpenError {}
