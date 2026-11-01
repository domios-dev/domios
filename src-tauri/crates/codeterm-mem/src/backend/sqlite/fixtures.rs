use crate::MemManager;
impl MemManager {
    #[cfg(test)]
    pub(crate) fn fixture_conn(&self) -> &parking_lot::Mutex<rusqlite::Connection> {
        self.store
            .fixture_backend()
            .downcast_ref::<super::SqliteMemStore>()
            .expect("SQLite test fixture")
            .conn
            .as_ref()
    }
    #[cfg(test)]
    pub(crate) fn try_conn_for(
        &self,
        timeout: std::time::Duration,
    ) -> Option<Box<dyn crate::backend::StorageLock + '_>> {
        self.fixture_conn()
            .try_lock_for(timeout)
            .map(|guard| Box::new(guard) as Box<dyn crate::backend::StorageLock>)
    }
    #[cfg(test)]
    pub(crate) fn with_conn<R>(&self, f: impl FnOnce(&rusqlite::Connection) -> R) -> R {
        f(&self.fixture_conn().lock())
    }
}
use std::{path::Path, sync::OnceLock};
pub fn seed_schema(data_dir: &Path) {
    static SCHEMA: OnceLock<Vec<u8>> = OnceLock::new();
    let bytes = SCHEMA.get_or_init(|| {
        crate::embeddings::ensure_extension_registered();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::store::init_schema(&conn).unwrap();
        crate::backend::sqlite::blueprint::metadata::initialize(&conn).unwrap();
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
    static SCHEMA: OnceLock<std::sync::Arc<parking_lot::Mutex<rusqlite::Connection>>> =
        OnceLock::new();
    let schema = SCHEMA.get_or_init(|| {
        super::SqliteMemStore::open_in_memory(
            &std::env::temp_dir().join(format!("mem-schema-{}", ulid::Ulid::new())),
        )
        .unwrap()
        .conn
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
impl crate::auto_extract::EvolutionObserver {
    pub(crate) fn observe_fixture(
        &self,
        conn: &rusqlite::Connection,
        scope: &str,
        observation: &str,
    ) -> Vec<String> {
        self.observe_evidence(scope, observation, |id| {
            codeterm_mem_store::graph::bump_evidence_count(conn, id).map_err(|e| e.to_string())
        })
    }
}

#[cfg(test)]
pub(crate) fn bare_connection() -> rusqlite::Connection {
    rusqlite::Connection::open_in_memory().unwrap()
}
