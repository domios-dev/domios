use rusqlite::Connection;
use crate::error::MemStoreResult;

pub fn run(conn: &Connection) -> MemStoreResult<()> {
    add_reason_column(conn)?;
    create_graph_purge_log(conn)?;
    add_evolution_columns(conn)?;
    add_promotion_threshold(conn)?;
    create_lesson_audit(conn)?;
    add_pin_column(conn)?;
    add_active_relations_unique_index(conn)?;
    Ok(())
}

fn add_reason_column(conn: &Connection) -> MemStoreResult<()> {
    let cols: Vec<String> = conn.prepare("PRAGMA table_info(relations)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(Result::ok).collect();
    if !cols.iter().any(|c| c == "reason") {
        conn.execute_batch("ALTER TABLE relations ADD COLUMN reason TEXT;")?;
    }
    Ok(())
}

fn add_evolution_columns(conn: &Connection) -> MemStoreResult<()> {
    let cols: Vec<String> = conn.prepare("PRAGMA table_info(mem_items)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(Result::ok).collect();
    if !cols.iter().any(|c| c == "evidence_count") {
        conn.execute_batch(
            "ALTER TABLE mem_items ADD COLUMN evidence_count INTEGER NOT NULL DEFAULT 1;",
        )?;
    }
    if !cols.iter().any(|c| c == "false_fire_count") {
        conn.execute_batch(
            "ALTER TABLE mem_items ADD COLUMN false_fire_count INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    Ok(())
}

fn add_promotion_threshold(conn: &Connection) -> MemStoreResult<()> {
    let cols: Vec<String> = conn.prepare("PRAGMA table_info(mem_items)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(Result::ok).collect();
    if !cols.iter().any(|c| c == "promotion_threshold") {
        conn.execute_batch("ALTER TABLE mem_items ADD COLUMN promotion_threshold INTEGER;")?;
    }
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

fn add_pin_column(conn: &Connection) -> MemStoreResult<()> {
    let cols: Vec<String> = conn.prepare("PRAGMA table_info(mem_items)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(Result::ok).collect();
    if !cols.iter().any(|c| c == "is_pinned_wakeup") {
        conn.execute_batch(
            "ALTER TABLE mem_items ADD COLUMN is_pinned_wakeup INTEGER NOT NULL DEFAULT 0;
             CREATE INDEX IF NOT EXISTS idx_mem_pinned ON mem_items(is_pinned_wakeup) WHERE is_pinned_wakeup = 1;",
        )?;
    }
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

// Dedup active edges + add a partial UNIQUE index so future INSERT OR IGNORE no-ops.
// Pre-existing UNIQUE(from_id, relation_type, to_id, valid_from) never fires because
// valid_from is a fresh timestamp on each write (issue #116). Historical (valid_to NOT NULL)
// rows are preserved as edge-history.
fn add_active_relations_unique_index(conn: &Connection) -> MemStoreResult<()> {
    let already: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type='index' AND name='idx_relations_active_unique'",
        [],
        |r| r.get(0),
    )?;
    if already > 0 {
        return Ok(());
    }

    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(
        "DELETE FROM relations \
         WHERE valid_to IS NULL AND rowid NOT IN ( \
             SELECT MIN(rowid) FROM relations \
             WHERE valid_to IS NULL \
             GROUP BY from_id, relation_type, to_id, scope \
         ); \
         CREATE UNIQUE INDEX IF NOT EXISTS idx_relations_active_unique \
           ON relations(from_id, relation_type, to_id, scope) \
           WHERE valid_to IS NULL;",
    )?;
    tx.commit()?;
    Ok(())
}
