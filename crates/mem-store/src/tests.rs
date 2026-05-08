use rusqlite::Connection;

fn init_v1_22_schema(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE relations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_id TEXT NOT NULL,
            relation_type TEXT NOT NULL,
            to_id TEXT NOT NULL,
            scope TEXT NOT NULL,
            valid_from TEXT NOT NULL,
            valid_to TEXT,
            confidence REAL NOT NULL DEFAULT 1.0,
            properties_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            UNIQUE(from_id, relation_type, to_id, valid_from)
        );
        CREATE TABLE entities (
            id TEXT PRIMARY KEY,
            entity_type TEXT NOT NULL,
            scope TEXT NOT NULL,
            properties_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE entity_aliases (
            alias_id TEXT PRIMARY KEY,
            canonical_id TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE TABLE mem_items (
            id TEXT PRIMARY KEY, uid TEXT NOT NULL UNIQUE,
            kind TEXT NOT NULL, scope TEXT NOT NULL,
            title TEXT NOT NULL, summary TEXT, area TEXT,
            tags_json TEXT NOT NULL DEFAULT '[]',
            body_inline TEXT, body_blob_path TEXT,
            source TEXT, importance REAL NOT NULL DEFAULT 3.0,
            created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
            accessed_at TEXT, access_count INTEGER NOT NULL DEFAULT 0,
            recurrence_count INTEGER NOT NULL DEFAULT 0
        );",
    ).unwrap();
}

#[test]
fn migration_adds_reason_column() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    let cols: Vec<String> = conn.prepare("PRAGMA table_info(relations)").unwrap()
        .query_map([], |r| r.get::<_, String>(1)).unwrap()
        .filter_map(Result::ok).collect();
    assert!(cols.contains(&"reason".to_string()),
            "expected `reason` column; got {:?}", cols);
}

#[test]
fn migration_creates_graph_purge_log() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='graph_purge_log'",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn migration_is_idempotent() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    crate::migrations::run(&conn).unwrap();
}
