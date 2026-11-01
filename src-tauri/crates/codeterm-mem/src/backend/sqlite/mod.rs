use crate::{backend::WriteContext, store};
use parking_lot::Mutex;
use rusqlite::Connection;
use std::{path::PathBuf, sync::Arc};
pub(crate) mod blueprint;
mod checkpoint;
mod construction;
pub use construction::ReadOnlyOpenError;
mod deletion;
pub mod engines;
#[cfg(any(test, feature = "test-support"))]
pub(crate) mod fixtures;
mod implementation;
mod operations;
mod save;
mod transfer;
#[cfg(any(test, feature = "test-support"))]
pub use crate::test_support::archive_mem_item_conn;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use operations::archive_item;
pub use operations::{bump_on_get, diary_page};

pub struct SqliteMemStore {
    pub(crate) conn: Arc<Mutex<Connection>>,
    pub(crate) blobs_dir: PathBuf,
}

impl SqliteMemStore {
    pub fn open_in_memory(blobs_dir: &std::path::Path) -> Result<Self, String> {
        crate::embeddings::ensure_extension_registered();
        std::fs::create_dir_all(blobs_dir).map_err(|e| e.to_string())?;
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        store::init_schema(&conn).map_err(|e| e.to_string())?;
        crate::backend::sqlite::blueprint::metadata::initialize(&conn)?;
        Ok(Self::from_connection(
            Arc::new(Mutex::new(conn)),
            blobs_dir.to_path_buf(),
        ))
    }

    fn query_under_conn(
        &self,
        text: &str,
        embedder: &std::sync::Arc<crate::embedder::ActiveEmbedder>,
    ) -> (
        Option<crate::embedder::StampedVector>,
        parking_lot::MutexGuard<'_, Connection>,
    ) {
        let embedding = embedder.query(text);
        let conn = self.conn.lock();
        if embedding
            .as_ref()
            .is_none_or(|vector| vector.identity == embedder.identity())
        {
            return (embedding, conn);
        }
        drop(conn);
        let embedding = embedder.query(text);
        (embedding, self.conn.lock())
    }

    pub(crate) fn from_connection(conn: Arc<Mutex<Connection>>, blobs_dir: PathBuf) -> Self {
        Self { conn, blobs_dir }
    }
}
