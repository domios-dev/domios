use rusqlite::Connection;

pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;
        PRAGMA synchronous = NORMAL;

        CREATE TABLE IF NOT EXISTS mem_items (
            id            TEXT PRIMARY KEY,
            uid           TEXT NOT NULL UNIQUE,
            kind          TEXT NOT NULL,
            scope         TEXT NOT NULL,
            title         TEXT NOT NULL,
            summary       TEXT,
            area          TEXT,
            tags_json     TEXT NOT NULL DEFAULT '[]',
            body_inline   TEXT,
            body_blob_path TEXT,
            source        TEXT,
            importance    REAL NOT NULL DEFAULT 3.0,
            created_at    TEXT NOT NULL,
            updated_at    TEXT NOT NULL,
            accessed_at   TEXT,
            access_count  INTEGER NOT NULL DEFAULT 0,
            recurrence_count INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_mem_kind ON mem_items(kind);
        CREATE INDEX IF NOT EXISTS idx_mem_scope ON mem_items(scope);
        CREATE INDEX IF NOT EXISTS idx_mem_area ON mem_items(area);
        CREATE INDEX IF NOT EXISTS idx_mem_importance ON mem_items(importance DESC);
        CREATE INDEX IF NOT EXISTS idx_mem_created ON mem_items(created_at DESC);

        CREATE VIRTUAL TABLE IF NOT EXISTS mem_fts USING fts5(
            mem_id UNINDEXED,
            title,
            summary,
            body,
            tags,
            tokenize = 'unicode61'
        );

        CREATE TABLE IF NOT EXISTS entities (
            id            TEXT PRIMARY KEY,
            entity_type   TEXT NOT NULL,
            scope         TEXT NOT NULL,
            properties_json TEXT NOT NULL DEFAULT '{}',
            created_at    TEXT NOT NULL,
            updated_at    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(entity_type);

        CREATE TABLE IF NOT EXISTS relations (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            from_id         TEXT NOT NULL,
            relation_type   TEXT NOT NULL,
            to_id           TEXT NOT NULL,
            scope           TEXT NOT NULL,
            valid_from      TEXT NOT NULL,
            valid_to        TEXT,
            confidence      REAL NOT NULL DEFAULT 1.0,
            properties_json TEXT NOT NULL DEFAULT '{}',
            created_at      TEXT NOT NULL,
            UNIQUE(from_id, relation_type, to_id, valid_from)
        );
        CREATE INDEX IF NOT EXISTS idx_relations_from ON relations(from_id);
        CREATE INDEX IF NOT EXISTS idx_relations_to ON relations(to_id);
        CREATE INDEX IF NOT EXISTS idx_relations_type ON relations(relation_type);
        CREATE INDEX IF NOT EXISTS idx_relations_valid ON relations(valid_from, valid_to);
        -- idx_relations_active_unique created by migrations::add_active_relations_unique_index
        -- after deduping pre-existing duplicate active edges.

        CREATE TABLE IF NOT EXISTS entity_aliases (
            alias_id     TEXT PRIMARY KEY,
            canonical_id TEXT NOT NULL,
            created_at   TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_aliases_canonical ON entity_aliases(canonical_id);

        -- Phase C: per-item audit trail. Every mutation (create/update/promote/
        -- invalidate/merge/delete) appends one row here. `changed_by` is the
        -- actor label set via `with_changed_by` (defaults to "user" if unset).
        CREATE TABLE IF NOT EXISTS mem_item_history (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            item_id         TEXT NOT NULL,
            changed_at      TEXT NOT NULL,
            changed_by      TEXT NOT NULL,
            change_type     TEXT NOT NULL,
            old_title       TEXT,
            new_title       TEXT,
            old_body        TEXT,
            new_body        TEXT,
            old_importance  REAL,
            new_importance  REAL,
            old_kind        TEXT,
            new_kind        TEXT,
            reason          TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_history_item ON mem_item_history(item_id, changed_at DESC);
        CREATE INDEX IF NOT EXISTS idx_history_changed_by ON mem_item_history(changed_by);
        "#,
    )?;

    // Pinned positions live in a dedicated table — only rows the user has explicitly anchored by drag-drop are persisted. Force-sim positions are
    // transient (worker-owned) and must not touch SQLite per tick.
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS graph_pinned_positions (
            mem_id      TEXT PRIMARY KEY,
            x           REAL NOT NULL,
            y           REAL NOT NULL,
            updated_at  INTEGER NOT NULL,
            FOREIGN KEY (mem_id) REFERENCES mem_items(id) ON DELETE CASCADE
        );
        "#,
    )?;

    // Legacy `graph_x` / `graph_y` columns are being retired — positions are transient. Drop them when running against SQLite ≥ 3.35 (supports
    // DROP COLUMN). Earlier versions just leave the dead columns in place.
    let has_graph_x: bool = {
        let mut stmt = conn.prepare("PRAGMA table_info(mem_items)")?;
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))?
            .filter_map(Result::ok)
            .collect();
        cols.iter().any(|c| c == "graph_x")
    };
    if has_graph_x {
        let _ = conn.execute_batch(
            "ALTER TABLE mem_items DROP COLUMN graph_x;\n\
             ALTER TABLE mem_items DROP COLUMN graph_y;",
        );
    }

    let has_access_count = {
        let mut stmt = conn.prepare("PRAGMA table_info(mem_items)")?;
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))?
            .filter_map(Result::ok)
            .collect();
        cols.iter().any(|c| c == "access_count")
    };
    if !has_access_count {
        conn.execute_batch(
            "ALTER TABLE mem_items ADD COLUMN access_count INTEGER NOT NULL DEFAULT 0;",
        )?;
    }

    let has_recurrence = {
        let mut stmt = conn.prepare("PRAGMA table_info(mem_items)")?;
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))?
            .filter_map(Result::ok)
            .collect();
        cols.iter().any(|c| c == "recurrence_count")
    };
    if !has_recurrence {
        conn.execute_batch(
            "ALTER TABLE mem_items ADD COLUMN recurrence_count INTEGER NOT NULL DEFAULT 0;",
        )?;
    }

    let has_source_path = {
        let mut stmt = conn.prepare("PRAGMA table_info(mem_items)")?;
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))?
            .filter_map(Result::ok)
            .collect();
        cols.iter().any(|c| c == "source_path")
    };
    if !has_source_path {
        conn.execute_batch(
            "ALTER TABLE mem_items ADD COLUMN source_path TEXT;\n\
             ALTER TABLE mem_items ADD COLUMN source_size INTEGER;\n\
             ALTER TABLE mem_items ADD COLUMN source_mtime TEXT;",
        )?;
    }

    codeterm_mem_store::migrations::run(conn)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

    Ok(())
}
