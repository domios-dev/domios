use std::path::Path;
#[cfg(all(test, feature = "embeddings"))]
use std::path::PathBuf;
use std::sync::OnceLock;

pub fn seed_schema(data_dir: &Path) {
    static SCHEMA: OnceLock<Vec<u8>> = OnceLock::new();
    let bytes = SCHEMA.get_or_init(|| {
        crate::embeddings::ensure_extension_registered();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::store::init_schema(&conn).unwrap();
        crate::solution_metadata::initialize(&conn).unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("schema.db");
        conn.execute("VACUUM INTO ?1", [path.to_str().unwrap()])
            .unwrap();
        std::fs::read(path).unwrap()
    });
    std::fs::create_dir_all(data_dir).unwrap();
    let path = data_dir.join("knowledge.db");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    std::io::Write::write_all(&mut file, bytes).unwrap();
}

#[cfg(feature = "test-support")]
pub(crate) fn memory_connection() -> Result<rusqlite::Connection, String> {
    static SCHEMA: OnceLock<parking_lot::Mutex<rusqlite::Connection>> = OnceLock::new();
    let schema = SCHEMA.get_or_init(|| {
        parking_lot::Mutex::new(crate::MemManager::empty_memory_connection().unwrap())
    });
    let source = schema.lock();
    let mut connection = rusqlite::Connection::open_in_memory().map_err(|e| e.to_string())?;
    {
        let backup =
            rusqlite::backup::Backup::new(&source, &mut connection).map_err(|e| e.to_string())?;
        assert_eq!(
            backup.step(-1).map_err(|e| e.to_string())?,
            rusqlite::backup::StepResult::Done
        );
    }
    connection
        .execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL;")
        .map_err(|e| e.to_string())?;
    Ok(connection)
}

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
