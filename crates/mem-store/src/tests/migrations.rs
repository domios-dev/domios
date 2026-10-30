use rusqlite::Connection;

use super::init_v1_22_schema;

#[test]
fn migration_adds_reason_column() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(relations)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(
        cols.contains(&"reason".to_string()),
        "expected `reason` column; got {:?}",
        cols
    );
}

#[test]
fn migration_creates_graph_purge_log() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='graph_purge_log'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn migration_is_idempotent() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    crate::migrations::run(&conn).unwrap();
}

#[test]
fn migration_dedups_active_relations_and_adds_partial_unique_index() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    conn.execute_batch(
        "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES ('a','X','global','{}','2026-01-01','2026-01-01'),
                   ('b','X','global','{}','2026-01-01','2026-01-01');
         INSERT INTO relations (from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at)
            VALUES ('a','r','b','global','2026-05-08T14:06:27Z',NULL,1.0,'{}','2026-05-08T14:06:27Z'),
                   ('a','r','b','global','2026-05-09T03:12:00Z',NULL,1.0,'{}','2026-05-09T03:12:00Z'),
                   ('a','r','b','global','2026-05-14T03:53:07Z',NULL,1.0,'{}','2026-05-14T03:53:07Z'),
                   ('a','r','b','global','2025-01-01',           '2025-06-01',1.0,'{}','2025-01-01'),
                   ('a','r','b','global','2025-07-01',           '2025-12-01',1.0,'{}','2025-07-01');",
    )
    .unwrap();

    let before_active: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM relations WHERE from_id='a' AND to_id='b' AND valid_to IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(before_active, 3, "fixture pre-condition: 3 active dup rows");

    crate::migrations::run(&conn).unwrap();

    let after_active: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM relations WHERE from_id='a' AND to_id='b' AND valid_to IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        after_active, 1,
        "migration keeps exactly one active row per (from,rel,to,scope)"
    );

    let history: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM relations WHERE from_id='a' AND to_id='b' AND valid_to IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        history, 2,
        "historical (invalidated) rows are preserved as edge history"
    );

    let now = chrono::Utc::now().to_rfc3339();
    let inserted = conn
        .execute(
            "INSERT OR IGNORE INTO relations (from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at)
         VALUES ('a','r','b','global',?1,NULL,1.0,'{}',?1)",
            rusqlite::params![now],
        )
        .unwrap();
    assert_eq!(
        inserted, 0,
        "INSERT OR IGNORE must no-op when an active row for the same key already exists"
    );
    let still_one_active: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM relations WHERE from_id='a' AND to_id='b' AND valid_to IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(still_one_active, 1);
}

#[test]
fn migration_adds_evolution_columns() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(mem_items)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(
        cols.contains(&"evidence_count".to_string()),
        "expected `evidence_count` column; got {:?}",
        cols
    );
    assert!(
        cols.contains(&"false_fire_count".to_string()),
        "expected `false_fire_count` column; got {:?}",
        cols
    );

    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, tags_json, created_at, updated_at)
         VALUES ('m_test','m_test','note','global','t','[]','2026-01-01','2026-01-01')",
        [],
    )
    .unwrap();
    let (ec, ff): (i64, i64) = conn
        .query_row(
            "SELECT evidence_count, false_fire_count FROM mem_items WHERE id = 'm_test'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(ec, 1, "evidence_count default must be 1");
    assert_eq!(ff, 0, "false_fire_count default must be 0");
}

#[test]
fn pin_column_added_by_migration() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(mem_items)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(
        cols.contains(&"is_pinned_wakeup".to_string()),
        "is_pinned_wakeup must be added by migration; got: {:?}",
        cols
    );
}

#[test]
fn pin_migration_is_idempotent() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    crate::migrations::run(&conn).unwrap();
    let pinned_cols: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('mem_items') WHERE name='is_pinned_wakeup'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        pinned_cols, 1,
        "running migrations twice must not duplicate the column"
    );
}

#[test]
fn pin_migration_creates_partial_index() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_mem_pinned'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1, "partial index idx_mem_pinned must exist");
    let sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='index' AND name='idx_mem_pinned'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        sql.to_lowercase().contains("where is_pinned_wakeup"),
        "expected partial index, got: {sql}"
    );
}
