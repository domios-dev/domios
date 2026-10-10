use super::*;
use crate::store::{init_schema, managed_payload, with_changed_by};

fn database() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let conn = Connection::open(dir.path().join("memory.db")).unwrap();
    init_schema(&conn).unwrap();
    (dir, conn)
}

fn seed(
    conn: &Connection,
    blobs: &Path,
    title: &str,
    area: &str,
    kind: MemKind,
    scope: MemScope,
) -> String {
    insert(
        conn,
        &NewMemItem {
            kind,
            scope,
            title: title.into(),
            summary: Some("kept summary".into()),
            area: Some(area.into()),
            tags: vec!["kept".into()],
            body: "kept body".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: true,
        },
        blobs,
    )
    .unwrap()
}

#[test]
fn audited_merge_preserves_payload_and_restores_one_item() {
    let (dir, conn) = database();
    let id = seed(
        &conn,
        dir.path(),
        "Scoped fact",
        "dev",
        MemKind::Fact,
        MemScope::Workspace("ws1".into()),
    );
    let second = seed(
        &conn,
        dir.path(),
        "Global rule",
        "dev",
        MemKind::Rule,
        MemScope::Global,
    );
    let (before, _) = managed_payload(&conn, &id, dir.path()).unwrap().unwrap();
    let outcome = with_changed_by("test-actor", || {
        merge_area(&conn, dir.path(), "dev", "development", false, None)
    })
    .unwrap();
    assert_eq!(
        outcome.counts,
        BTreeMap::from([("fact".into(), 1), ("rule".into(), 1)])
    );
    for item_id in [&id, &second] {
        let history = get_item_history(&conn, item_id).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[1].change_type, "update");
        assert_eq!(history[1].changed_by, "test-actor");
        assert_eq!(history[1].changed_fields, vec!["area"]);
        let change: AreaChange = serde_json::from_str(
            history[1]
                .reason
                .as_deref()
                .unwrap()
                .strip_prefix(MERGE_PREFIX)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(change.from.as_deref(), Some("dev"));
        assert_eq!(change.into, "development");
    }
    let (mut after, _) = managed_payload(&conn, &id, dir.path()).unwrap().unwrap();
    after.area = before.area.clone();
    assert_eq!(after, before);
    assert_eq!(
        restore_area(&conn, dir.path(), &id, None)
            .unwrap()
            .as_deref(),
        Some("dev")
    );
    assert_eq!(
        managed_payload(&conn, &id, dir.path()).unwrap().unwrap().0,
        before
    );
    assert_eq!(
        managed_payload(&conn, &second, dir.path())
            .unwrap()
            .unwrap()
            .0
            .area
            .as_deref(),
        Some("development")
    );
    assert_eq!(get_item_history(&conn, &id).unwrap().len(), 3);
    assert!(restore_area(&conn, dir.path(), &id, None)
        .unwrap()
        .is_none());
}

#[test]
fn dry_run_is_read_only_and_reports_each_kind() {
    let (dir, conn) = database();
    let id = seed(
        &conn,
        dir.path(),
        "Fact",
        "dev",
        MemKind::Fact,
        MemScope::Global,
    );
    seed(
        &conn,
        dir.path(),
        "Note",
        "dev",
        MemKind::Note,
        MemScope::Global,
    );
    let changes = conn.total_changes();
    let out = merge_area(&conn, dir.path(), "dev", "new-name", true, Some("run")).unwrap();
    assert_eq!(
        out.counts,
        BTreeMap::from([("fact".into(), 1), ("note".into(), 1)])
    );
    assert_eq!(conn.total_changes(), changes);
    assert_eq!(get_item_history(&conn, &id).unwrap().len(), 1);
}

#[test]
fn refuses_reserved_same_empty_and_missing_areas() {
    let (dir, conn) = database();
    seed(
        &conn,
        dir.path(),
        "Fact",
        "dev",
        MemKind::Fact,
        MemScope::Global,
    );
    for (from, into) in [
        ("_dreams", "dev"),
        ("dev", "_graph"),
        ("dev", "dev"),
        ("", "dev"),
        ("dev", ""),
        ("missing", "dev"),
        (" _graph", "dev"),
    ] {
        for dry_run in [true, false] {
            assert!(
                merge_area(&conn, dir.path(), from, into, dry_run, None).is_err(),
                "{from:?} -> {into:?}"
            );
        }
    }
}

#[test]
fn audit_failure_rolls_back_all_items() {
    let (dir, conn) = database();
    let first = seed(
        &conn,
        dir.path(),
        "A",
        "dev",
        MemKind::Fact,
        MemScope::Global,
    );
    let last = seed(
        &conn,
        dir.path(),
        "Z",
        "dev",
        MemKind::Fact,
        MemScope::Global,
    );
    conn.execute_batch("CREATE TRIGGER deny_audit BEFORE INSERT ON mem_item_history WHEN NEW.item_id = 'fact_z' BEGIN SELECT RAISE(ABORT, 'audit unavailable'); END;").unwrap();
    assert!(merge_area(&conn, dir.path(), "dev", "development", false, None).is_err());
    for id in [&first, &last] {
        assert_eq!(
            managed_payload(&conn, id, dir.path())
                .unwrap()
                .unwrap()
                .0
                .area
                .as_deref(),
            Some("dev")
        );
        assert_eq!(get_item_history(&conn, id).unwrap().len(), 1);
    }
}

#[test]
fn dream_run_cap_counts_merges_not_items_and_writes_diary() {
    let (dir, conn) = database();
    for index in 0..4 {
        let area = format!("area{index}");
        seed(
            &conn,
            dir.path(),
            &format!("Fact {index}"),
            &area,
            MemKind::Fact,
            MemScope::Global,
        );
        let out = merge_area(&conn, dir.path(), &area, "keeper", false, Some("run-one"));
        assert_eq!(out.is_ok(), index < DREAMER_AREA_MERGE_CAP);
    }
    let count: usize = conn.query_row("SELECT COUNT(*) FROM mem_items WHERE area='_dreams' AND source='dreamer-area-merge:run-one'", [], |r| r.get(0)).unwrap();
    assert_eq!(count, DREAMER_AREA_MERGE_CAP);
    conn.execute("DELETE FROM mem_items WHERE area='_dreams'", [])
        .unwrap();
    assert!(merge_area(&conn, dir.path(), "area3", "keeper", false, Some("run-one")).is_err());
    assert!(merge_area(&conn, dir.path(), "area3", "keeper", true, Some("run-one")).is_ok());
    assert!(merge_area(&conn, dir.path(), "area3", "keeper", false, Some("run-two")).is_ok());
}

#[test]
fn area_update_never_reads_or_rewrites_a_file_body() {
    let (dir, conn) = database();
    let id = seed(
        &conn,
        dir.path(),
        "File fact",
        "dev",
        MemKind::Fact,
        MemScope::Global,
    );
    let file = dir.path().join("source.md");
    std::fs::write(&file, "new disk text").unwrap();
    conn.execute(
        "UPDATE mem_items SET source=?1 WHERE id=?2",
        params![format!("file:{}", file.display()), id],
    )
    .unwrap();
    merge_area(&conn, dir.path(), "dev", "development", false, None).unwrap();
    let (body, accesses): (String, usize) = conn
        .query_row(
            "SELECT body_inline, access_count FROM mem_items WHERE id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(body, "kept body");
    assert_eq!(accesses, 0);
}

#[test]
fn explicit_version_restores_area_after_a_body_edit() {
    let (dir, conn) = database();
    let id = seed(
        &conn,
        dir.path(),
        "Fact",
        "dev",
        MemKind::Fact,
        MemScope::Global,
    );
    merge_area(&conn, dir.path(), "dev", "development", false, None).unwrap();
    let version = get_item_history(&conn, &id).unwrap()[1].id;
    update(
        &conn,
        &id,
        &MemUpdate {
            body: Some("edited body".into()),
            ..Default::default()
        },
        dir.path(),
    )
    .unwrap();
    assert!(restore_area(&conn, dir.path(), &id, None)
        .unwrap()
        .is_none());
    assert_eq!(
        restore_area(&conn, dir.path(), &id, Some(version))
            .unwrap()
            .as_deref(),
        Some("dev")
    );
    assert_eq!(
        managed_payload(&conn, &id, dir.path())
            .unwrap()
            .unwrap()
            .0
            .body,
        "edited body"
    );
}
