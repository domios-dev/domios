use rusqlite::Connection;

mod cluster;

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

#[test]
fn canonicalize_relation_types_renames_and_drops_duplicates() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES ('a','X','global','{}','2026-01-01','2026-01-01'),
                   ('b','X','global','{}','2026-01-01','2026-01-01'),
                   ('c','X','global','{}','2026-01-01','2026-01-01');
         INSERT INTO relations (from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at)
            VALUES ('a','references','b','global','2026-01-01',NULL,1.0,'{}','2026-01-01'),
                   ('a','mentions','b','global','2026-01-01',NULL,1.0,'{}','2026-01-01'),
                   ('a','references','c','global','2026-01-01',NULL,1.0,'{}','2026-01-01');"
    ).unwrap();

    let n = crate::graph::canonicalize_relation_types(
        &conn,
        &[("references", "mentions")],
    ).unwrap();
    assert!(n >= 2, "must report renames + drops, got {}", n);

    let still_references: i64 = conn.query_row(
        "SELECT COUNT(*) FROM relations WHERE relation_type='references' AND valid_to IS NULL",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(still_references, 0, "no leftover 'references' rows after canonicalize");

    let mentions_a_b: i64 = conn.query_row(
        "SELECT COUNT(*) FROM relations WHERE from_id='a' AND to_id='b' AND relation_type='mentions' AND valid_to IS NULL",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(mentions_a_b, 1, "the (a,b) pair survives once after dup-drop");

    let mentions_a_c: i64 = conn.query_row(
        "SELECT COUNT(*) FROM relations WHERE from_id='a' AND to_id='c' AND relation_type='mentions' AND valid_to IS NULL",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(mentions_a_c, 1, "(a,c) gets renamed to mentions");
}

#[test]
fn list_entities_with_degree_counts_active_relations_only() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES ('hub','Hub','global','{}','2026-01-01','2026-01-01'),
                   ('a','X','global','{}','2026-01-01','2026-01-01'),
                   ('b','X','global','{}','2026-01-01','2026-01-01'),
                   ('lonely','X','global','{}','2026-01-01','2026-01-01');
         INSERT INTO relations (from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at)
            VALUES ('hub','r1','a','global','2026-01-01',NULL,1.0,'{}','2026-01-01'),
                   ('hub','r2','b','global','2026-01-01',NULL,1.0,'{}','2026-01-01'),
                   ('hub','r3','a','global','2025-01-01','2025-06-01',1.0,'{}','2025-01-01');"
    ).unwrap();

    let mut entities = crate::graph::list_entities_with_degree(&conn).unwrap();
    entities.sort_by(|a, b| a.id.cmp(&b.id));
    let by_id: std::collections::HashMap<String, &crate::graph::EntityDegree> =
        entities.iter().map(|e| (e.id.clone(), e)).collect();

    assert_eq!(entities.len(), 4);
    assert_eq!(by_id.get("hub").unwrap().degree, 2,
        "invalidated edge must NOT count");
    assert_eq!(by_id.get("a").unwrap().degree, 1);
    assert_eq!(by_id.get("b").unwrap().degree, 1);
    assert_eq!(by_id.get("lonely").unwrap().degree, 0);
    assert_eq!(by_id.get("hub").unwrap().entity_type, "Hub");
}

#[test]
fn cleanup_orphan_entities_skips_protected_types_and_recent_entities() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    let old = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
    let recent = (chrono::Utc::now() - chrono::Duration::days(1)).to_rfc3339();
    conn.execute(
        "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES ('orphan_old','person','global','{}',?1,?1),
                   ('orphan_recent','person','global','{}',?2,?2),
                   ('protected_old','kind','global','{}',?1,?1),
                   ('connected_old','person','global','{}',?1,?1),
                   ('peer','person','global','{}',?1,?1);",
        rusqlite::params![&old, &recent],
    ).unwrap();
    conn.execute(
        "INSERT INTO relations (from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at)
            VALUES ('connected_old','knows','peer','global',?1,NULL,1.0,'{}',?1);",
        rusqlite::params![&old],
    ).unwrap();

    let cutoff = (chrono::Utc::now() - chrono::Duration::days(14)).to_rfc3339();
    let n = crate::graph::cleanup_orphan_entities(
        &conn,
        &["kind", "area", "agent", "workspace"],
        &cutoff,
    ).unwrap();
    assert_eq!(n, 1, "only 'orphan_old' should be deleted");

    let exists = |id: &str| -> bool {
        let c: i64 = conn.query_row(
            "SELECT COUNT(*) FROM entities WHERE id=?1", rusqlite::params![id], |r| r.get(0),
        ).unwrap();
        c > 0
    };
    assert!(!exists("orphan_old"), "old orphan deleted");
    assert!(exists("orphan_recent"), "recent orphan kept");
    assert!(exists("protected_old"), "protected entity_type kept");
    assert!(exists("connected_old"), "entity with active relation kept");
}

#[test]
fn invalidate_broken_relations_targets_only_truly_broken() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES ('a','X','global','{}','2026-01-01','2026-01-01');
         INSERT INTO mem_items (id,uid,kind,scope,title,tags_json,created_at,updated_at)
            VALUES ('m1','m1','note','global','t','[]','2026-01-01','2026-01-01');
         INSERT INTO relations (from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at)
            VALUES ('a','okrel','m1','global','2026-01-01',NULL,1.0,'{}','2026-01-01'),
                   ('a','badrel','ghost','global','2026-01-01',NULL,1.0,'{}','2026-01-01'),
                   ('ghost','badrel2','m1','global','2026-01-01',NULL,1.0,'{}','2026-01-01');"
    ).unwrap();
    let now = chrono::Utc::now().to_rfc3339();
    let n = crate::graph::invalidate_broken_relations(&conn, &now).unwrap();
    assert_eq!(n, 2);
    let active: i64 = conn.query_row(
        "SELECT COUNT(*) FROM relations WHERE valid_to IS NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(active, 1, "only the (a,m1) row stays active");
}

#[test]
fn add_alias_with_rewire_merges_relations_and_deletes_alias() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES ('canonical','person','global','{}','2026-01-01','2026-01-01'),
                   ('alias','person','global','{}','2026-01-01','2026-01-01'),
                   ('peer1','thing','global','{}','2026-01-01','2026-01-01'),
                   ('peer2','thing','global','{}','2026-01-01','2026-01-01');
         INSERT INTO relations (from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at)
            VALUES ('alias','knows','peer1','global','2026-01-01',NULL,1.0,'{}','2026-01-01'),
                   ('peer2','knows','alias','global','2026-01-01',NULL,1.0,'{}','2026-01-01'),
                   ('canonical','knows','peer1','global','2026-01-01',NULL,1.0,'{}','2026-01-01');"
    ).unwrap();

    crate::graph::add_alias_with_rewire(&conn, "alias", "canonical").unwrap();

    let alias_entity: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entities WHERE id='alias'", [], |r| r.get(0)).unwrap();
    assert_eq!(alias_entity, 0, "alias entity deleted");

    let alias_row: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entity_aliases WHERE alias_id='alias' AND canonical_id='canonical'",
        [], |r| r.get(0)).unwrap();
    assert_eq!(alias_row, 1, "alias mapping recorded");

    let leftover: i64 = conn.query_row(
        "SELECT COUNT(*) FROM relations WHERE from_id='alias' OR to_id='alias'",
        [], |r| r.get(0)).unwrap();
    assert_eq!(leftover, 0, "no relations reference alias_id after rewire");

    let peer2_to_canonical: i64 = conn.query_row(
        "SELECT COUNT(*) FROM relations WHERE from_id='peer2' AND to_id='canonical'",
        [], |r| r.get(0)).unwrap();
    assert_eq!(peer2_to_canonical, 1, "peer2->alias rewired to peer2->canonical");

    let canonical_to_peer1: i64 = conn.query_row(
        "SELECT COUNT(*) FROM relations WHERE from_id='canonical' AND to_id='peer1'",
        [], |r| r.get(0)).unwrap();
    assert_eq!(canonical_to_peer1, 1, "duplicate alias->peer1 dropped, canonical->peer1 survives once");
}

#[test]
fn add_alias_with_rewire_rejects_self_alias() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    let err = crate::graph::add_alias_with_rewire(&conn, "x", "x").unwrap_err();
    assert!(matches!(err, crate::error::MemStoreError::InvalidArgument(_)));
}

