use rusqlite::params;

use super::super::*;
use super::common::fresh;

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
fn update_body_replace_retains_old_body_in_history() {
    let (conn, blobs) = fresh();
    let id = insert(
        &conn,
        &NewMemItem {
            kind: MemKind::Skill,
            scope: MemScope::Global,
            title: "skill doc".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "---\nname: skill\n---\noriginal long body".into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        },
        &blobs,
    )
    .unwrap();

    // Destructive full replace with a tiny body.
    let replaced = update(
        &conn,
        &id,
        &MemUpdate {
            body: Some("oops".into()),
            ..Default::default()
        },
        &blobs,
    )
    .unwrap();
    assert!(replaced);
    assert_eq!(get(&conn, &id, &blobs).unwrap().unwrap().body, "oops");

    // The prior body must be recoverable from history.
    let history = get_item_history(&conn, &id).unwrap();
    let last = history.last().unwrap();
    assert_eq!(
        last.old_body.as_deref(),
        Some("---\nname: skill\n---\noriginal long body")
    );
    assert_eq!(last.new_body.as_deref(), Some("oops"));
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
