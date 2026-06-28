use rusqlite::{params, Connection};

use super::super::*;
use super::common::fresh;

#[test]
fn schema_initializes_idempotently() {
    let conn = Connection::open_in_memory().unwrap();
    init_schema(&conn).unwrap();
    init_schema(&conn).unwrap();
}

#[test]
fn migration_creates_pinned_positions_idempotently() {
    let conn = Connection::open_in_memory().unwrap();
    init_schema(&conn).unwrap();
    init_schema(&conn).unwrap();
    // mem_items must NOT carry layout columns anymore — positions live in graph_pinned_positions or in the force-sim worker.
    let mem_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(mem_items)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(!mem_cols.iter().any(|c| c == "graph_x"));
    assert!(!mem_cols.iter().any(|c| c == "graph_y"));
    // The pinned-positions table must exist with the expected columns.
    let pin_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(graph_pinned_positions)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(pin_cols.iter().any(|c| c == "mem_id"));
    assert!(pin_cols.iter().any(|c| c == "x"));
    assert!(pin_cols.iter().any(|c| c == "y"));
    assert!(pin_cols.iter().any(|c| c == "updated_at"));
}

#[test]
fn graph_full_returns_nodes_and_edges() {
    let (conn, blobs) = fresh();
    let a = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Skill,
            scope: MemScope::Global,
            title: "alpha".into(),
            summary: Some("first".into()),
            area: Some("core".into()),
            tags: vec!["t1".into()],
            body: "body alpha".into(),
            source: None,
            importance: 4.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();
    let b = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Script,
            scope: MemScope::Global,
            title: "beta".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "body beta".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();
    crate::mem::graph::insert_relation(
        &conn,
        &crate::mem::graph::Relation {
            from_id: a.clone(),
            relation_type: "has_script".into(),
            to_id: b.clone(),
            scope: "global".into(),
            valid_from: chrono::Utc::now().to_rfc3339(),
            valid_to: None,
            confidence: 1.0,
            properties: serde_json::json!({}),
            reason: None,
        },
    )
    .unwrap();

    let dump = graph_full(&conn).unwrap();
    assert_eq!(dump.stats.node_count, 2);
    assert_eq!(dump.stats.edge_count, 1);
    assert!(dump
        .nodes
        .iter()
        .any(|n| n.id == a && n.kind == "skill" && n.has_body));
    assert!(dump.nodes.iter().any(|n| n.id == b));
    let edge = &dump.edges[0];
    assert_eq!(edge.from, a);
    assert_eq!(edge.to, b);
    assert_eq!(edge.rel, "has_script");

    // pinned positions: set, read, unset, read-again.
    let existed = set_pinned_position(&conn, &a, 12.5, -3.25).unwrap();
    assert!(existed, "mem_item exists → upsert must report true");
    let got = list_pinned_positions(&conn).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].mem_id, a);
    assert!((got[0].x - 12.5).abs() < 1e-9);
    assert!((got[0].y - (-3.25)).abs() < 1e-9);

    // unknown mem_id → returns false, nothing written
    let existed2 = set_pinned_position(&conn, "nope", 1.0, 2.0).unwrap();
    assert!(!existed2);
    assert_eq!(list_pinned_positions(&conn).unwrap().len(), 1);

    let removed = delete_pinned_position(&conn, &a).unwrap();
    assert_eq!(removed, 1);
    assert!(list_pinned_positions(&conn).unwrap().is_empty());

    // MemItem no longer carries positions — body/metadata unchanged.
    let item = get(&conn, &a, &blobs).unwrap().unwrap();
    assert_eq!(item.id, a);
}

#[test]
fn stats_counts_by_kind() {
    let (conn, blobs) = fresh();
    for (i, kind) in [MemKind::Fact, MemKind::Fact, MemKind::Skill]
        .iter()
        .enumerate()
    {
        insert(
            &conn,
            &NewMemItem {
                kind: *kind,
                scope: MemScope::Global,
                title: format!("item {}", i),
                summary: None,
                area: None,
                tags: vec![],
                body: "b".into(),
                source: None,
                importance: 3.0,
                source_path: None,
                pinned: false,
            },
            &blobs,
        )
        .unwrap();
    }
    let s = stats(&conn).unwrap();
    assert_eq!(s.total_items, 3);
    assert_eq!(s.by_kind.get("fact"), Some(&2));
    assert_eq!(s.by_kind.get("skill"), Some(&1));
}

#[test]
fn taxonomy_groups_by_area_and_kind() {
    let (conn, blobs) = fresh();
    let mk = |kind: MemKind, title: &str, area: Option<&str>| {
        insert(
            &conn,
            &NewMemItem {
                kind,
                scope: MemScope::Global,
                title: title.into(),
                summary: None,
                area: area.map(String::from),
                tags: vec![],
                body: "b".into(),
                source: None,
                importance: 3.0,
                source_path: None,
                pinned: false,
            },
            &blobs,
        )
        .unwrap()
    };
    mk(MemKind::Fact, "f1", Some("deploy"));
    mk(MemKind::Fact, "f2", Some("deploy"));
    mk(MemKind::Skill, "s1", Some("deploy"));
    mk(MemKind::Rule, "r1", None);
    mk(MemKind::Preference, "p1", None);

    let tax = get_taxonomy(&conn, None).unwrap();
    let deploy = tax.get("deploy").expect("deploy bucket");
    assert_eq!(deploy.get("fact"), Some(&2));
    assert_eq!(deploy.get("skill"), Some(&1));
    let none_bucket = tax.get("(none)").expect("(none) bucket");
    assert_eq!(none_bucket.get("rule"), Some(&1));
    assert_eq!(none_bucket.get("preference"), Some(&1));
}

#[test]
fn graph_full_excludes_invalidated_relations() {
    let conn = Connection::open_in_memory().unwrap();
    init_schema(&conn).unwrap();

    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, tags_json, created_at, updated_at)
         VALUES ('m_a','m_a','note','global','A','[]',?1,?1),
                ('m_b','m_b','note','global','B','[]',?1,?1)",
        params![now],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json, created_at)
         VALUES ('m_a','active_rel','m_b','global','2026-01-01T00:00:00.000000+00:00', NULL, 1.0, '{}', '2026-01-01T00:00:00')",
        [],
    )
    .unwrap();

    // SQLite's datetime('now') uses space separator; stored ISO uses 'T' (0x54 > 0x20).
    let today: String = conn
        .query_row("SELECT date('now')", [], |r| r.get(0))
        .unwrap();
    let dead_valid_to = format!("{}T00:00:00.000000+00:00", today);
    conn.execute(
        "INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json, created_at)
         VALUES ('m_a','dead_rel','m_b','global','2024-01-01T00:00:00.000000+00:00', ?1, 1.0, '{}', '2024-01-01T00:00:00')",
        params![dead_valid_to],
    )
    .unwrap();

    let dump = graph_full(&conn).unwrap();
    let rels: Vec<&str> = dump.edges.iter().map(|e| e.rel.as_str()).collect();
    assert!(
        rels.contains(&"active_rel"),
        "active relation must be present"
    );
    assert!(
        !rels.contains(&"dead_rel"),
        "invalidated relation must be filtered out"
    );
}
