use super::super::*;
use super::common::{fresh, mk_item};

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
