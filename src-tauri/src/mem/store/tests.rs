use std::path::PathBuf;

use rusqlite::{params, Connection};

use super::*;

fn fresh() -> (Connection, PathBuf) {
    let conn = Connection::open_in_memory().unwrap();
    init_schema(&conn).unwrap();
    let tmp = std::env::temp_dir().join(format!("memstore-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&tmp).unwrap();
    (conn, tmp)
}

#[test]
fn schema_initializes_idempotently() {
    let conn = Connection::open_in_memory().unwrap();
    init_schema(&conn).unwrap();
    init_schema(&conn).unwrap();
}

#[test]
fn insert_and_get_inline() {
    let (conn, blobs) = fresh();
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "Test Fact".into(),
            summary: Some("a fact".into()),
            area: None,
            tags: vec!["a".into(), "b".into()],
            body: "short body".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();
    assert_eq!(id, "fact_test_fact");
    let got = get(&conn, &id, &blobs).unwrap().unwrap();
    assert_eq!(got.body, "short body");
    assert_eq!(got.tags, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn get_updates_accessed_at_and_access_count() {
    let (conn, blobs) = fresh();
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "touch me".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "body".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();

    // Sleep 1 ms so rfc3339 timestamps differ even on fast CI hardware.
    std::thread::sleep(std::time::Duration::from_millis(2));

    let got = get(&conn, &id, &blobs).unwrap().unwrap();
    let created = got.created_at.clone();
    let accessed = got
        .accessed_at
        .clone()
        .expect("accessed_at should be set after get");
    assert!(
        accessed > created,
        "accessed_at ({}) must be newer than created_at ({})",
        accessed,
        created
    );
    assert_eq!(got.access_count, 1, "first get bumps access_count to 1");

    // Second get bumps again.
    std::thread::sleep(std::time::Duration::from_millis(2));
    let got2 = get(&conn, &id, &blobs).unwrap().unwrap();
    assert_eq!(got2.access_count, 2);
    assert!(got2.accessed_at.unwrap() > accessed);
}

#[test]
fn insert_blob_when_body_large() {
    let (conn, blobs) = fresh();
    let big = "x".repeat(5000);
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Note,
            scope: MemScope::Global,
            title: "big".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: big.clone(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();
    let got = get(&conn, &id, &blobs).unwrap().unwrap();
    assert_eq!(got.body.len(), big.len());
}

#[test]
fn update_partial_only_changes_provided_fields() {
    let (conn, blobs) = fresh();
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "original title".into(),
            summary: Some("original summary".into()),
            area: Some("original".into()),
            tags: vec!["a".into(), "b".into()],
            body: "original body".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();

    // Update only tags + importance
    let updated = update(
        &conn,
        &id,
        &MemUpdate {
            title: None,
            summary: None,
            area: None,
            tags: Some(vec!["x".into(), "y".into(), "z".into()]),
            body: None,
            importance: Some(4.5),
            bump_recurrence: None,
            pinned: None,
        },
        &blobs,
    )
    .unwrap();
    assert!(updated);

    let after = get(&conn, &id, &blobs).unwrap().unwrap();
    assert_eq!(after.title, "original title"); // unchanged
    assert_eq!(after.summary.unwrap(), "original summary"); // unchanged
    assert_eq!(after.area.unwrap(), "original"); // unchanged
    assert_eq!(after.body, "original body"); // unchanged
    assert_eq!(after.tags, vec!["x", "y", "z"]); // changed
    assert_eq!(after.importance, 4.5); // changed
}

#[test]
fn update_sets_is_pinned_wakeup() {
    let (conn, blobs) = fresh();
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "pin me".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "body".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();

    let updated = update(
        &conn,
        &id,
        &MemUpdate {
            pinned: Some(true),
            ..Default::default()
        },
        &blobs,
    )
    .unwrap();
    assert!(updated);

    let pinned: i64 = conn
        .query_row(
            "SELECT is_pinned_wakeup FROM mem_items WHERE id = ?1",
            params![&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pinned, 1);
}

#[test]
fn update_can_unpin() {
    let (conn, blobs) = fresh();
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "unpin me".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "body".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();

    update(
        &conn,
        &id,
        &MemUpdate {
            pinned: Some(true),
            ..Default::default()
        },
        &blobs,
    )
    .unwrap();
    update(
        &conn,
        &id,
        &MemUpdate {
            pinned: Some(false),
            ..Default::default()
        },
        &blobs,
    )
    .unwrap();

    let pinned: i64 = conn
        .query_row(
            "SELECT is_pinned_wakeup FROM mem_items WHERE id = ?1",
            params![&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pinned, 0);
}

#[test]
fn update_pinned_only_does_not_touch_other_fields() {
    let (conn, blobs) = fresh();
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "keep fields".into(),
            summary: Some("a summary".into()),
            area: Some("core".into()),
            tags: vec!["t1".into(), "t2".into()],
            body: "untouched body".into(),
            source: None,
            importance: 4.25,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();

    update(
        &conn,
        &id,
        &MemUpdate {
            pinned: Some(true),
            ..Default::default()
        },
        &blobs,
    )
    .unwrap();

    let after = get(&conn, &id, &blobs).unwrap().unwrap();
    assert_eq!(after.title, "keep fields");
    assert_eq!(after.summary.as_deref(), Some("a summary"));
    assert_eq!(after.area.as_deref(), Some("core"));
    assert_eq!(after.body, "untouched body");
    assert_eq!(after.tags, vec!["t1", "t2"]);
    assert!(
        (after.importance - 4.25).abs() < 1e-9,
        "pinning must not bump importance"
    );
}

#[test]
fn pin_only_update_emits_history_row() {
    let (conn, blobs) = fresh();
    let id = with_changed_by("user", || {
        insert(
            &conn,
            &NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: "audit pin".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: "body".into(),
                source: None,
                importance: 3.0,
                source_path: None,
                pinned: false,
            },
            &blobs,
        )
        .unwrap()
    });

    let baseline = get_item_history(&conn, &id).unwrap().len();

    with_changed_by("dreamer-calibration:test", || {
        update(
            &conn,
            &id,
            &MemUpdate {
                pinned: Some(true),
                ..Default::default()
            },
            &blobs,
        )
        .unwrap();
    });

    let after = get_item_history(&conn, &id).unwrap();
    assert_eq!(
        after.len(),
        baseline + 1,
        "pin-only update must emit one history row"
    );
    let row = after.last().unwrap();
    assert_eq!(row.change_type, "update");
    assert_eq!(row.changed_by, "dreamer-calibration:test");
}

#[test]
fn update_returns_false_for_unknown_id() {
    let (conn, blobs) = fresh();
    let r = update(
        &conn,
        "nonexistent",
        &MemUpdate {
            title: Some("x".into()),
            summary: None,
            area: None,
            tags: None,
            body: None,
            importance: None,
            bump_recurrence: None,
            pinned: None,
        },
        &blobs,
    )
    .unwrap();
    assert!(!r);
}

#[test]
fn update_body_refreshes_fts() {
    let (conn, blobs) = fresh();
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "x".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "the quick brown fox".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();
    update(
        &conn,
        &id,
        &MemUpdate {
            title: None,
            summary: None,
            area: None,
            tags: None,
            body: Some("changed to grey wolf".into()),
            importance: None,
            bump_recurrence: None,
            pinned: None,
        },
        &blobs,
    )
    .unwrap();
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mem_fts WHERE mem_id = ?1 AND body LIKE '%grey wolf%'",
            params![&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn upsert_replaces_existing_in_place() {
    let (conn, blobs) = fresh();
    let mk = |body: &str| NewMemItem {
        kind: MemKind::Preference,
        scope: MemScope::Global,
        title: "identity".into(),
        summary: None,
        area: None,
        tags: vec![],
        body: body.into(),
        source: None,
        importance: 5.0,
        source_path: None,
        pinned: false,
    };
    let id1 = insert(&conn, &mk("placeholder"), &blobs).unwrap();
    assert_eq!(id1, "preference_identity");
    // Plain insert collides → hash suffix
    let id2 = insert(&conn, &mk("real identity"), &blobs).unwrap();
    assert_ne!(id1, id2);
    // Upsert collapses back to the canonical id
    let id3 = upsert(&conn, &mk("updated identity"), &blobs).unwrap();
    assert_eq!(id3, "preference_identity");
    let got = get(&conn, "preference_identity", &blobs).unwrap().unwrap();
    assert_eq!(got.body, "updated identity");
}

#[test]
fn slug_collision_appends_hash() {
    let (conn, blobs) = fresh();
    let mk = |body: &str| NewMemItem {
        kind: MemKind::Fact,
        scope: MemScope::Global,
        title: "Same Title".into(),
        summary: None,
        area: None,
        tags: vec![],
        body: body.into(),
        source: None,
        importance: 3.0,
        source_path: None,
        pinned: false,
    };
    let a = insert(&conn, &mk("body a"), &blobs).unwrap();
    let b = insert(&conn, &mk("body b"), &blobs).unwrap();
    assert_ne!(a, b);
    assert_eq!(a, "fact_same_title");
    assert!(b.starts_with("fact_same_title_"));
}

#[test]
fn delete_removes_from_fts() {
    let (conn, blobs) = fresh();
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Note,
            scope: MemScope::Global,
            title: "to delete".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "x".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();
    assert!(delete(&conn, &id).unwrap());
    assert!(get(&conn, &id, &blobs).unwrap().is_none());
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mem_fts WHERE mem_id = ?1",
            params![&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn migration_creates_pinned_positions_idempotently() {
    let conn = Connection::open_in_memory().unwrap();
    init_schema(&conn).unwrap();
    init_schema(&conn).unwrap();
    // mem_items must NOT carry layout columns anymore — positions live
    // in graph_pinned_positions or in the force-sim worker.
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
    let (mut conn, blobs) = fresh();
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

// ── Phase C: history ──────────────────────────────────────────────

fn mk_item(title: &str, body: &str) -> NewMemItem {
    NewMemItem {
        kind: MemKind::Learning,
        scope: MemScope::Global,
        title: title.into(),
        summary: None,
        area: Some("test".into()),
        tags: vec![],
        body: body.into(),
        source: None,
        importance: 2.0,
        source_path: None,
        pinned: false,
    }
}

#[test]
fn history_logs_create_update_delete_events() {
    let (conn, blobs) = fresh();

    // create
    let id = with_changed_by("user", || {
        insert(&conn, &mk_item("hist one", "original body"), &blobs)
    })
    .unwrap();

    // update title, body, importance
    with_changed_by("user", || {
        update(
            &conn,
            &id,
            &MemUpdate {
                title: Some("hist one v2".into()),
                summary: None,
                area: None,
                tags: None,
                body: Some("new body".into()),
                importance: Some(4.5),
                bump_recurrence: None,
                pinned: None,
            },
            &blobs,
        )
        .unwrap();
    });

    // delete
    with_changed_by("worker:extract", || {
        delete(&conn, &id).unwrap();
    });

    let history = get_item_history(&conn, &id).unwrap();
    assert_eq!(history.len(), 3);

    assert_eq!(history[0].change_type, "create");
    assert_eq!(history[0].changed_by, "user");
    assert_eq!(history[0].new_title.as_deref(), Some("hist one"));
    assert!(history[0].old_title.is_none());

    assert_eq!(history[1].change_type, "update");
    assert_eq!(history[1].old_title.as_deref(), Some("hist one"));
    assert_eq!(history[1].new_title.as_deref(), Some("hist one v2"));
    assert_eq!(history[1].old_body.as_deref(), Some("original body"));
    assert_eq!(history[1].new_body.as_deref(), Some("new body"));
    assert_eq!(history[1].old_importance, Some(2.0));
    assert_eq!(history[1].new_importance, Some(4.5));

    assert_eq!(history[2].change_type, "delete");
    assert_eq!(history[2].changed_by, "worker:extract");
    assert_eq!(history[2].old_title.as_deref(), Some("hist one v2"));
    assert!(history[2].new_title.is_none());
}

#[test]
fn history_merge_records_winner_reference() {
    let (conn, blobs) = fresh();
    let a = insert(&conn, &mk_item("dup a", "body a"), &blobs).unwrap();
    let b = insert(&conn, &mk_item("dup b", "body b"), &blobs).unwrap();

    // simulate the merge path — loser `a` collapses into winner `b`
    delete_with_reason(&conn, &a, Some("merge_into=item_b")).unwrap();
    let _ = b;

    let history = get_item_history(&conn, &a).unwrap();
    let merge = history
        .iter()
        .find(|h| h.change_type == "merge")
        .expect("merge row");
    assert_eq!(merge.reason.as_deref(), Some("merge_into=item_b"));
    assert_eq!(merge.old_title.as_deref(), Some("dup a"));
}

#[test]
fn history_changed_by_thread_local_defaults_to_user() {
    let (conn, blobs) = fresh();
    let id = insert(&conn, &mk_item("default actor", "body"), &blobs).unwrap();
    let history = get_item_history(&conn, &id).unwrap();
    assert_eq!(history[0].changed_by, "user");
}

// ── Phase D: taxonomy ─────────────────────────────────────────────

#[test]
fn taxonomy_groups_by_area_and_kind() {
    let (conn, blobs) = fresh();
    let mut mk = |kind: MemKind, title: &str, area: Option<&str>| {
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

// ── mem tail ─────────────────────────────────────────────────────

#[test]
fn tail_recent_returns_items_newest_first() {
    let (conn, blobs) = fresh();
    let a = with_changed_by("user", || {
        insert(&conn, &mk_item("tail a", "a"), &blobs).unwrap()
    });
    std::thread::sleep(std::time::Duration::from_millis(2));
    let b = with_changed_by("dreaming:rem", || {
        insert(&conn, &mk_item("tail b", "b"), &blobs).unwrap()
    });

    let rows = tail_recent(&conn, None, 10).unwrap();
    assert_eq!(rows.len(), 2);
    // newest-first
    assert_eq!(rows[0].id, b);
    assert_eq!(rows[1].id, a);
    assert_eq!(rows[0].changed_by, "dreaming:rem");
    assert_eq!(rows[1].changed_by, "user");
}

#[test]
fn insert_with_file_prefix_source_path_populates_source_size_and_mtime() {
    let (conn, blobs) = fresh();
    let dir = std::env::temp_dir().join(format!("memstore-skill-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let skill_md = dir.join("SKILL.md");
    std::fs::write(&skill_md, "# Test skill\n\nbody content here").unwrap();

    let source_with_prefix = format!("file:{}", skill_md.display());
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Skill,
            scope: MemScope::Global,
            title: "test prefix skill".into(),
            summary: None,
            area: None,
            tags: vec!["folder-skill".into()],
            body: "# Test skill\n\nbody content here".into(),
            source: Some(source_with_prefix.clone()),
            importance: 4.0,
            source_path: Some(source_with_prefix),
            pinned: false,
        },
        &blobs,
    )
    .unwrap();

    let (size, mtime): (Option<i64>, Option<String>) = conn
        .query_row(
            "SELECT source_size, source_mtime FROM mem_items WHERE id = ?1",
            params![&id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let expected_size = std::fs::metadata(&skill_md).unwrap().len() as i64;
    assert_eq!(
        size,
        Some(expected_size),
        "source_size must be populated on insert when source_path uses file: prefix"
    );
    assert!(
        mtime.is_some(),
        "source_mtime must be populated on insert when source_path uses file: prefix"
    );
}

#[test]
fn get_bumps_updated_at_when_source_file_changes() {
    let (conn, blobs) = fresh();
    let dir = std::env::temp_dir().join(format!("memstore-getbump-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let skill_md = dir.join("SKILL.md");
    std::fs::write(&skill_md, "# v1\n\noriginal").unwrap();

    let source = format!("file:{}", skill_md.display());
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Skill,
            scope: MemScope::Global,
            title: "bump skill".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "# v1\n\noriginal".into(),
            source: Some(source.clone()),
            importance: 4.0,
            source_path: Some(source),
            pinned: false,
        },
        &blobs,
    )
    .unwrap();

    let initial_updated_at: String = conn
        .query_row(
            "SELECT updated_at FROM mem_items WHERE id = ?1",
            params![&id],
            |r| r.get(0),
        )
        .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(10));

    let new_body = "# v2\n\nedited content longer to change size";
    std::fs::write(&skill_md, new_body).unwrap();

    let got = get(&conn, &id, &blobs).unwrap().unwrap();
    assert_eq!(got.body, new_body, "body must be re-read from disk");
    assert!(
        got.updated_at > initial_updated_at,
        "updated_at must bump when source file content changes (initial={}, after_get={})",
        initial_updated_at,
        got.updated_at,
    );
}

#[test]
fn get_skips_disk_check_when_recently_accessed() {
    let (conn, blobs) = fresh();
    let dir = std::env::temp_dir().join(format!("memstore-debounce-{}", ulid::Ulid::new()));
    std::fs::create_dir_all(&dir).unwrap();
    let skill_md = dir.join("SKILL.md");
    std::fs::write(&skill_md, "# v1\n\noriginal").unwrap();

    let source = format!("file:{}", skill_md.display());
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Skill,
            scope: MemScope::Global,
            title: "debounce skill".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "# v1\n\noriginal".into(),
            source: Some(source.clone()),
            importance: 4.0,
            source_path: Some(source),
            pinned: false,
        },
        &blobs,
    )
    .unwrap();

    // First get: warms accessed_at and metadata fields.
    let _ = get(&conn, &id, &blobs).unwrap().unwrap();
    let after_first: String = conn
        .query_row(
            "SELECT updated_at FROM mem_items WHERE id = ?1",
            params![&id],
            |r| r.get(0),
        )
        .unwrap();

    // Mutate file on disk WITHIN the debounce window.
    std::fs::write(&skill_md, "# v2\n\nedited within debounce window").unwrap();

    // Immediate re-get — debounce skips the metadata check; updated_at stays.
    let got = get(&conn, &id, &blobs).unwrap().unwrap();
    assert_eq!(
        got.updated_at, after_first,
        "updated_at must NOT bump within the 10s debounce window — was {} → {}",
        after_first, got.updated_at,
    );
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

#[test]
fn insert_with_pinned_true_sets_is_pinned_wakeup() {
    let (conn, blobs) = fresh();
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "pinned-fact".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "body".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: true,
        },
        &blobs,
    )
    .unwrap();
    let pinned: i64 = conn
        .query_row(
            "SELECT is_pinned_wakeup FROM mem_items WHERE id = ?1",
            params![&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pinned, 1, "pinned=true must persist on insert");
}

#[test]
fn insert_with_pinned_false_remains_unpinned() {
    let (conn, blobs) = fresh();
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "regular".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "body".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();
    let pinned: i64 = conn
        .query_row(
            "SELECT is_pinned_wakeup FROM mem_items WHERE id = ?1",
            params![&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pinned, 0);
}

#[test]
fn list_pinned_returns_only_pinned_items_ordered_by_importance() {
    let (conn, blobs) = fresh();

    let id_a = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "high".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "hi".into(),
            source: None,
            importance: 5.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();
    update(
        &conn,
        &id_a,
        &MemUpdate {
            pinned: Some(true),
            ..Default::default()
        },
        &blobs,
    )
    .unwrap();

    let id_b = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "low".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "lo".into(),
            source: None,
            importance: 2.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();
    update(
        &conn,
        &id_b,
        &MemUpdate {
            pinned: Some(true),
            ..Default::default()
        },
        &blobs,
    )
    .unwrap();

    insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Fact,
            scope: MemScope::Global,
            title: "unpinned".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "x".into(),
            source: None,
            importance: 5.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();

    let pinned = list_pinned(&conn, &blobs, 100).unwrap();
    assert_eq!(
        pinned.len(),
        2,
        "exactly 2 pinned items, got {}",
        pinned.len()
    );
    assert_eq!(pinned[0].title, "high", "high importance first");
    assert_eq!(pinned[1].title, "low");
}

#[test]
fn list_pinned_respects_limit() {
    let (conn, blobs) = fresh();

    for i in 0..10 {
        let id = insert(
            &conn,
            &NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: format!("item-{i}"),
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
        update(
            &conn,
            &id,
            &MemUpdate {
                pinned: Some(true),
                ..Default::default()
            },
            &blobs,
        )
        .unwrap();
    }
    let pinned = list_pinned(&conn, &blobs, 3).unwrap();
    assert_eq!(pinned.len(), 3, "limit must cap result");
}
