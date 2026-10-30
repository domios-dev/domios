use crate::error::{MemStoreError, MemStoreResult};
use rusqlite::Connection;

pub const SCHEMA_VERSION: i64 = 2;

pub fn run(conn: &Connection) -> MemStoreResult<()> {
    let tx = conn.unchecked_transaction()?;
    let stamped: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='mem_schema_version')",
        [], |row| row.get(0),
    )?;
    if stamped {
        let versions = tx.prepare("SELECT version FROM mem_schema_version")?
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if versions != [SCHEMA_VERSION] {
            return Err(MemStoreError::InvalidArgument(format!(
                "unsupported memory schema version {versions:?}; expected {SCHEMA_VERSION}; older schemas cannot be upgraded"
            )));
        }
    } else {
        let occupied: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name NOT LIKE 'sqlite_%')",
            [], |row| row.get(0),
        )?;
        if occupied {
            return Err(MemStoreError::InvalidArgument("unstamped memory schema refused; create a fresh database".into()));
        }
        tx.execute_batch(include_str!("baseline.sql"))?;
        tx.execute_batch(include_str!("author_guards.sql"))?;
        create_graph_purge_log(&tx)?;
        create_lesson_audit(&tx)?;
        crate::skill_folders::create_tombstone_table(&tx)?;
        tx.execute_batch("CREATE TABLE mem_schema_version(version INTEGER NOT NULL); INSERT INTO mem_schema_version VALUES(2);")?;
    }
    tx.commit()?;
    Ok(())
}

fn create_lesson_audit(conn: &Connection) -> MemStoreResult<()> {
    conn.execute_batch(
        r#"CREATE TABLE IF NOT EXISTS lesson_audit (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            lesson_uid  TEXT NOT NULL,
            machine_id  TEXT NOT NULL,
            action      TEXT NOT NULL CHECK(action IN ('promoted','demoted')),
            ts          TEXT NOT NULL,
            reason      TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_lesson_audit_uid ON lesson_audit(lesson_uid, ts DESC);"#,
    )?;
    Ok(())
}

fn create_graph_purge_log(conn: &Connection) -> MemStoreResult<()> {
    conn.execute_batch(
        r#"CREATE TABLE IF NOT EXISTS graph_purge_log (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            purged_at   TEXT NOT NULL,
            count       INTEGER NOT NULL,
            scope       TEXT,
            before      TEXT,
            reason_filter TEXT,
            actor       TEXT NOT NULL,
            details_json TEXT NOT NULL DEFAULT '{}'
        );
        CREATE INDEX IF NOT EXISTS idx_purge_at ON graph_purge_log(purged_at DESC);"#,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
