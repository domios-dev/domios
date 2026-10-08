use rusqlite::{params, Connection, OptionalExtension};

use super::super::{ClosedStructure, ClosedStructureView, StructureState};
use super::ClosedTabsStore;

impl ClosedTabsStore {
    pub(super) fn init_structures(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS closed_structure (
            id TEXT PRIMARY KEY, closed_at INTEGER NOT NULL, state TEXT NOT NULL,
            payload TEXT NOT NULL, summary TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS closed_structure_recent ON closed_structure(closed_at DESC);",
        )
    }

    pub fn structure(&self, id: &str) -> Result<Option<ClosedStructure>, String> {
        let json: Option<String> = self
            .conn
            .lock()
            .query_row(
                "SELECT payload FROM closed_structure WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        json.map(|json| serde_json::from_str(&json).map_err(|error| error.to_string()))
            .transpose()
    }

    pub fn pending_structures(&self) -> Result<Vec<ClosedStructure>, String> {
        let conn = self.conn.lock();
        let mut statement = conn
            .prepare("SELECT payload FROM closed_structure WHERE state = 'captured' ORDER BY closed_at DESC, rowid DESC")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?;
        rows.map(|row| {
            serde_json::from_str(&row.map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())
        })
        .collect()
    }

    pub fn save_structure(&self, record: &ClosedStructure) -> Result<(), String> {
        let json = serde_json::to_string(record).map_err(|error| error.to_string())?;
        let summary = serde_json::to_string(&record.view()).map_err(|error| error.to_string())?;
        let state = serde_json::to_value(record.state).map_err(|error| error.to_string())?;
        self.conn.lock().execute(
            "INSERT INTO closed_structure (id, closed_at, state, payload, summary) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET state = excluded.state, payload = excluded.payload, summary = excluded.summary",
            params![record.id, record.closed_at as i64, state.as_str(), json, summary]
        ).map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn finish_structure_close(&self, id: &str) -> Result<(), String> {
        let mut record = self.structure(id)?.ok_or("Close inventory missing")?;
        record.state = StructureState::Closed;
        let json = serde_json::to_string(&record).map_err(|error| error.to_string())?;
        let summary = serde_json::to_string(&record.view()).map_err(|error| error.to_string())?;
        let mut conn = self.conn.lock();
        let transaction = conn.transaction().map_err(|error| error.to_string())?;
        for item in &record.tabs {
            transaction
                .execute(
                    "DELETE FROM closed_tab WHERE tab_id = ?1 AND closed_at >= ?2",
                    params![item.tab.id, record.closed_at as i64],
                )
                .map_err(|error| error.to_string())?;
        }
        transaction
            .execute(
                "UPDATE closed_structure SET state = 'closed', payload = ?2, summary = ?3 WHERE id = ?1",
                params![id, json, summary],
            )
            .map_err(|error| error.to_string())?;
        transaction.execute("DELETE FROM closed_group WHERE NOT EXISTS (SELECT 1 FROM closed_tab WHERE closed_tab.group_id = closed_group.group_id)", [])
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())
    }

    pub fn discard_structure(&self, id: &str) -> Result<(), String> {
        self.conn
            .lock()
            .execute("DELETE FROM closed_structure WHERE id = ?1", params![id])
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn structure_views(&self) -> Result<Vec<ClosedStructureView>, String> {
        let conn = self.conn.lock();
        let mut statement = conn.prepare("SELECT summary FROM closed_structure WHERE state <> 'captured' ORDER BY closed_at DESC, rowid DESC LIMIT 100")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?;
        rows.map(|row| {
            serde_json::from_str(&row.map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())
        })
        .collect()
    }
}
