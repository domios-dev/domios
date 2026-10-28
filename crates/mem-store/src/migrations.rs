use crate::error::MemStoreResult;
use rusqlite::Connection;

pub const SCHEMA_VERSION: i64 = 2;

type Migration = fn(&Connection) -> MemStoreResult<()>;

pub fn run(conn: &Connection) -> MemStoreResult<()> {
    run_steps(conn, &[baseline, authorship])
}

fn run_steps(conn: &Connection, steps: &[Migration]) -> MemStoreResult<()> {
    for (index, step) in steps.iter().enumerate() {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS mem_schema_version (version INTEGER NOT NULL);
            INSERT INTO mem_schema_version(version) SELECT 0 WHERE NOT EXISTS (SELECT 1 FROM mem_schema_version);")?;
        let version: i64 =
            tx.query_row("SELECT version FROM mem_schema_version", [], |r| r.get(0))?;
        let next = index as i64 + 1;
        if version < next {
            step(&tx)?;
            tx.execute("UPDATE mem_schema_version SET version=?1", [next])?;
        }
        tx.commit()?;
    }
    Ok(())
}

fn columns(conn: &Connection, table: &str) -> MemStoreResult<Vec<String>> {
    Ok(conn
        .prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |r| r.get(1))?
        .collect::<Result<_, _>>()?)
}

fn baseline(conn: &Connection) -> MemStoreResult<()> {
    conn.execute_batch(include_str!("baseline.sql"))?;
    let existing = columns(conn, "mem_items")?;
    for column in ["graph_x", "graph_y"] {
        if existing.iter().any(|c| c == column) {
            conn.execute_batch(&format!("ALTER TABLE mem_items DROP COLUMN {column}"))?;
        }
    }
    for (name, definition) in [
        ("access_count", "INTEGER NOT NULL DEFAULT 0"),
        ("recurrence_count", "INTEGER NOT NULL DEFAULT 0"),
        ("source_path", "TEXT"),
        ("source_size", "INTEGER"),
        ("source_mtime", "TEXT"),
        ("evidence_count", "INTEGER NOT NULL DEFAULT 1"),
        ("false_fire_count", "INTEGER NOT NULL DEFAULT 0"),
        ("promotion_threshold", "INTEGER"),
        ("is_pinned_wakeup", "INTEGER NOT NULL DEFAULT 0"),
        ("seed_fingerprint", "TEXT"),
    ] {
        if !existing.iter().any(|c| c == name) {
            conn.execute_batch(&format!(
                "ALTER TABLE mem_items ADD COLUMN {name} {definition}"
            ))?;
        }
    }
    if !columns(conn, "relations")?.iter().any(|c| c == "reason") {
        conn.execute_batch("ALTER TABLE relations ADD COLUMN reason TEXT")?;
    }
    create_graph_purge_log(conn)?;
    create_lesson_audit(conn)?;
    crate::skill_folders::create_tombstone_table(conn)?;
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_mem_pinned ON mem_items(is_pinned_wakeup) WHERE is_pinned_wakeup = 1;
        DELETE FROM relations WHERE valid_to IS NULL AND rowid NOT IN (
            SELECT MIN(rowid) FROM relations WHERE valid_to IS NULL GROUP BY from_id,relation_type,to_id,scope);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_relations_active_unique ON relations(from_id,relation_type,to_id,scope) WHERE valid_to IS NULL;")?;
    Ok(())
}

fn authorship(conn: &Connection) -> MemStoreResult<()> {
    for table in ["mem_items", "mem_item_history"] {
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN author_kind TEXT NOT NULL DEFAULT 'system' CHECK(author_kind IN ('owner','neighbor','system'));
            ALTER TABLE {table} ADD COLUMN author_uid TEXT;
            ALTER TABLE {table} ADD COLUMN author_friendship TEXT;
            ALTER TABLE {table} ADD COLUMN author_label TEXT DEFAULT 'unscoped';
            UPDATE {table} SET author_kind='owner',author_label=NULL;"))?;
    }
    conn.execute_batch(include_str!("author_guards.sql"))?;
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
