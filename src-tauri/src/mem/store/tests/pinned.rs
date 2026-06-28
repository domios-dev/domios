use rusqlite::params;

use super::super::*;
use super::common::fresh;

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
