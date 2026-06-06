mod cron_view;
mod crud;
mod schema;
#[cfg(test)]
mod tests;
mod types;

pub use cron_view::list_local;
pub use schema::{init_schema, migrate_schema, SCHEMA_VERSION};
pub use types::*;

use parking_lot::Mutex;
use rusqlite::Connection;
use std::path::Path;
use std::sync::Arc;

pub struct ServicesStore {
    conn: Arc<Mutex<Connection>>,
}

impl ServicesStore {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute("PRAGMA foreign_keys = ON", [])?;
        init_schema(&conn)?;
        migrate_schema(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    #[cfg(test)]
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute("PRAGMA foreign_keys = ON", [])?;
        init_schema(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub(crate) fn conn(&self) -> Arc<Mutex<Connection>> {
        self.conn.clone()
    }
}
