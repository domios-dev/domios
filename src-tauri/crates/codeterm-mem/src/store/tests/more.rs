use super::*;

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

#[test]
fn history_logs_create_update_delete_events() {
    let (conn, blobs) = fresh();

    let id = with_changed_by("user", || {
        insert(&conn, &mk_item("hist one", "original body"), &blobs)
    })
    .unwrap();

    with_changed_by("worker:extract", || {
        update(
            &conn,
            &id,
            &MemUpdate {
                title: Some("hist one v2".into()),
                summary: Some("new summary".into()),
                area: Some("updated-area".into()),
                tags: Some(vec!["updated-tag".into()]),
                body: Some("new body".into()),
                importance: Some(4.5),
                bump_recurrence: None,
                pinned: None,
            },
            &blobs,
        )
        .unwrap();
    });

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
    assert_eq!(history[1].changed_by, "worker:extract");
    assert_eq!(
        history[1].changed_fields,
        vec!["title", "body", "importance", "summary", "area", "tags"]
    );
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
