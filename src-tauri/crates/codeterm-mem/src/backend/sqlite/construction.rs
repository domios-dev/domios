use super::*;
use std::path::Path;
impl SqliteMemStore {
    pub fn open(db_path: &Path, blobs_dir: &Path) -> Result<Self, String> {
        crate::embeddings::ensure_extension_registered();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::create_dir_all(blobs_dir).map_err(|e| e.to_string())?;
        let conn = Connection::open(db_path).map_err(|e| format!("open knowledge.db: {e}"))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        store::init_schema(&conn).map_err(|e| format!("init schema: {e}"))?;
        crate::backend::sqlite::blueprint::cleanup::remove_v1(&conn, blobs_dir)?;
        crate::backend::sqlite::blueprint::metadata::initialize(&conn)?;
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
