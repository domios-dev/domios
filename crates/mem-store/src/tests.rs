use rusqlite::Connection;

pub(crate) fn init_v1_22_schema(conn: &Connection) {
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

fn seed_two_invalidated_one_active(conn: &Connection) {
    conn.execute_batch(
        "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES ('a','X','global','{}','2026-01-01','2026-01-01'),
                   ('b','X','global','{}','2026-01-01','2026-01-01');
         INSERT INTO relations (from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at)
            VALUES ('a','r1','b','global','2026-01-01',NULL,1.0,'{}','2026-01-01'),
                   ('a','r2','b','global','2025-01-01','2025-06-01',1.0,'{}','2025-01-01'),
                   ('a','r3','b','global','2024-01-01','2024-06-01',1.0,'{}','2024-01-01');"
    ).unwrap();
}

#[test]
fn purge_invalidated_dry_run_reports_count_without_deleting() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    seed_two_invalidated_one_active(&conn);

    let report = crate::graph::purge_invalidated(&conn, crate::graph::PurgeOptions {
        before: None, reason_filter: None, dry_run: true, actor: "test".into(),
    }).unwrap();
    assert_eq!(report.count, 2);
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM relations", [], |r| r.get(0)).unwrap();
    assert_eq!(total, 3, "dry-run must NOT delete");
    let log_rows: i64 = conn.query_row("SELECT COUNT(*) FROM graph_purge_log", [], |r| r.get(0)).unwrap();
    assert_eq!(log_rows, 0, "dry-run must NOT write audit row");
}

#[test]
fn purge_invalidated_actually_deletes_and_audits() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    seed_two_invalidated_one_active(&conn);

    let report = crate::graph::purge_invalidated(&conn, crate::graph::PurgeOptions {
        before: None, reason_filter: None, dry_run: false, actor: "test".into(),
    }).unwrap();
    assert_eq!(report.count, 2);

    let total: i64 = conn.query_row("SELECT COUNT(*) FROM relations", [], |r| r.get(0)).unwrap();
    assert_eq!(total, 1, "active row only");
    let log_count: i64 = conn.query_row("SELECT count FROM graph_purge_log", [], |r| r.get(0)).unwrap();
    assert_eq!(log_count, 2);
}

#[test]
fn purge_invalidated_filters_by_reason_substring() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES ('a','X','global','{}','2026-01-01','2026-01-01'),
                   ('b','X','global','{}','2026-01-01','2026-01-01');
         INSERT INTO relations (from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at,reason)
            VALUES ('a','r1','b','global','2025-01-01','2025-06-01',1.0,'{}','2025-01-01','broken-rel-cleanup'),
                   ('a','r2','b','global','2025-01-01','2025-06-01',1.0,'{}','2025-01-01','manual-merge');"
    ).unwrap();

    let report = crate::graph::purge_invalidated(&conn, crate::graph::PurgeOptions {
        before: None,
        reason_filter: Some("broken".into()),
        dry_run: false,
        actor: "test".into(),
    }).unwrap();
    assert_eq!(report.count, 1);
    let remaining: i64 = conn.query_row("SELECT COUNT(*) FROM relations", [], |r| r.get(0)).unwrap();
    assert_eq!(remaining, 1);
    let kept_reason: String = conn.query_row(
        "SELECT reason FROM relations LIMIT 1", [], |r| r.get(0)).unwrap();
    assert_eq!(kept_reason, "manual-merge");
}

#[test]
fn route_invalidates_direct_creates_two_segments() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES ('a','X','global','{}','2026-01-01','2026-01-01'),
                   ('b','X','global','{}','2026-01-01','2026-01-01'),
                   ('c','Topic','global','{}','2026-01-01','2026-01-01');
         INSERT INTO relations (from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at)
            VALUES ('a','about','b','global','2026-01-01',NULL,1.0,'{}','2026-01-01');"
    ).unwrap();

    let report = crate::graph::route(&conn, crate::graph::RouteOptions {
        from: "a".into(), to: "b".into(), via: "c".into(),
        rel_in: None, rel_out: None, reason: Some("topic-introduce".into()),
    }).unwrap();
    assert_eq!(report.direct_invalidated, 1);
    assert_eq!(report.edges_created, 2);

    let direct_active: i64 = conn.query_row(
        "SELECT COUNT(*) FROM relations WHERE from_id='a' AND to_id='b' AND valid_to IS NULL",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(direct_active, 0);
    let segments_active: i64 = conn.query_row(
        "SELECT COUNT(*) FROM relations WHERE valid_to IS NULL AND ((from_id='a' AND to_id='c') OR (from_id='c' AND to_id='b'))",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(segments_active, 2);

    let invalidated_reason: String = conn.query_row(
        "SELECT reason FROM relations WHERE from_id='a' AND to_id='b' AND valid_to IS NOT NULL",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(invalidated_reason, "topic-introduce");
}

#[test]
fn route_returns_not_found_when_no_active_direct_edge() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES ('a','X','global','{}','2026-01-01','2026-01-01'),
                   ('b','X','global','{}','2026-01-01','2026-01-01'),
                   ('c','Topic','global','{}','2026-01-01','2026-01-01');"
    ).unwrap();

    let err = crate::graph::route(&conn, crate::graph::RouteOptions {
        from: "a".into(), to: "b".into(), via: "c".into(),
        rel_in: None, rel_out: None, reason: None,
    }).unwrap_err();
    assert!(matches!(err, crate::error::MemStoreError::NotFound(_)));
}

#[test]
fn revive_broken_restores_relations_with_existing_endpoints() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES ('a','X','global','{}','2026-01-01','2026-01-01'),
                   ('b','X','global','{}','2026-01-01','2026-01-01');
         INSERT INTO relations (from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at)
            VALUES ('a','revive_me','b','global','2026-01-01','2026-02-01',1.0,'{}','2026-01-01'),
                   ('a','keep_dead','ghost','global','2026-01-01','2026-02-01',1.0,'{}','2026-01-01');"
    ).unwrap();

    let report = crate::graph::revive_broken(&conn, crate::graph::ReviveBrokenOptions {
        since: None, dry_run: false,
    }).unwrap();
    assert_eq!(report.revived, 1);

    let revived: i64 = conn.query_row(
        "SELECT COUNT(*) FROM relations WHERE relation_type='revive_me' AND valid_to IS NULL",
        [], |r| r.get(0)).unwrap();
    assert_eq!(revived, 1);
    let still_dead: i64 = conn.query_row(
        "SELECT COUNT(*) FROM relations WHERE relation_type='keep_dead' AND valid_to IS NULL",
        [], |r| r.get(0)).unwrap();
    assert_eq!(still_dead, 0, "ghost endpoint must keep relation invalidated");

    let reason: String = conn.query_row(
        "SELECT reason FROM relations WHERE relation_type='revive_me'", [], |r| r.get(0)).unwrap();
    assert_eq!(reason, "revive-broken");
}

#[test]
fn revive_broken_is_idempotent() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES ('a','X','global','{}','2026-01-01','2026-01-01'),
                   ('b','X','global','{}','2026-01-01','2026-01-01');
         INSERT INTO relations (from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at)
            VALUES ('a','r','b','global','2026-01-01','2026-02-01',1.0,'{}','2026-01-01');"
    ).unwrap();

    let r1 = crate::graph::revive_broken(&conn, crate::graph::ReviveBrokenOptions {
        since: None, dry_run: false,
    }).unwrap();
    assert_eq!(r1.revived, 1);
    let r2 = crate::graph::revive_broken(&conn, crate::graph::ReviveBrokenOptions {
        since: None, dry_run: false,
    }).unwrap();
    assert_eq!(r2.revived, 0, "already-active rows must not match");
}

#[test]
fn introduce_topic_creates_topic_routes_sources_invalidates_direct() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES ('anchor','Hub','global','{}','2026-01-01','2026-01-01'),
                   ('s1','Source','global','{}','2026-01-01','2026-01-01'),
                   ('s2','Source','global','{}','2026-01-01','2026-01-01');
         INSERT INTO relations (from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at)
            VALUES ('s1','about','anchor','global','2026-01-01',NULL,1.0,'{}','2026-01-01'),
                   ('s2','about','anchor','global','2026-01-01',NULL,1.0,'{}','2026-01-01');"
    ).unwrap();

    let report = crate::graph::introduce_topic(&conn, crate::graph::IntroduceTopicOptions {
        topic_id: "topic_test".into(), entity_type: "Topic".into(),
        sources: vec!["s1".into(), "s2".into()], anchor: "anchor".into(),
        rel_source: "about".into(), rel_anchor: "about".into(),
        reason: Some("e2e".into()),
    }).unwrap();
    assert!(report.topic_created);
    assert_eq!(report.edges_created, 3);
    assert_eq!(report.edges_invalidated, 2);

    let topic_exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entities WHERE id='topic_test'", [], |r| r.get(0)).unwrap();
    assert_eq!(topic_exists, 1);

    let s1_anchor_active: i64 = conn.query_row(
        "SELECT COUNT(*) FROM relations WHERE from_id='s1' AND to_id='anchor' AND valid_to IS NULL",
        [], |r| r.get(0)).unwrap();
    assert_eq!(s1_anchor_active, 0);
    let topic_anchor_active: i64 = conn.query_row(
        "SELECT COUNT(*) FROM relations WHERE from_id='topic_test' AND to_id='anchor' AND valid_to IS NULL",
        [], |r| r.get(0)).unwrap();
    assert_eq!(topic_anchor_active, 1);
}

#[test]
fn introduce_topic_conflicts_when_existing_entity_has_different_type() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES ('topic_existing','Document','global','{}','2026-01-01','2026-01-01'),
                   ('anchor','Hub','global','{}','2026-01-01','2026-01-01'),
                   ('s1','Source','global','{}','2026-01-01','2026-01-01');"
    ).unwrap();
    let err = crate::graph::introduce_topic(&conn, crate::graph::IntroduceTopicOptions {
        topic_id: "topic_existing".into(),
        entity_type: "Topic".into(),
        sources: vec!["s1".into()],
        anchor: "anchor".into(),
        rel_source: "about".into(),
        rel_anchor: "about".into(),
        reason: None,
    }).expect_err("must conflict");
    let msg = format!("{}", err);
    assert!(msg.contains("topic_existing"), "msg: {}", msg);
    assert!(msg.contains("Document"), "msg: {}", msg);
    assert!(msg.contains("Topic"), "msg: {}", msg);
}

#[test]
fn introduce_topic_handles_existing_entity_idempotently() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES ('anchor','Hub','global','{}','2026-01-01','2026-01-01'),
                   ('topic_existing','Topic','global','{}','2026-01-01','2026-01-01'),
                   ('s1','Source','global','{}','2026-01-01','2026-01-01');
         INSERT INTO relations (from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at)
            VALUES ('s1','about','anchor','global','2026-01-01',NULL,1.0,'{}','2026-01-01');"
    ).unwrap();

    let report = crate::graph::introduce_topic(&conn, crate::graph::IntroduceTopicOptions {
        topic_id: "topic_existing".into(), entity_type: "Topic".into(),
        sources: vec!["s1".into()], anchor: "anchor".into(),
        rel_source: "about".into(), rel_anchor: "about".into(),
        reason: None,
    }).unwrap();
    assert!(!report.topic_created, "existing entity must NOT be re-created");
    assert_eq!(report.edges_invalidated, 1);
    assert_eq!(report.edges_created, 2);
}
