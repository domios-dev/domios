use chrono::Utc;
use rusqlite::{params, Connection};

use super::*;

fn open_test_conn() -> (Connection, tempfile::TempDir) {
    let conn = Connection::open_in_memory().unwrap();
    crate::mem::store::init_schema(&conn).unwrap();
    (conn, tempfile::tempdir().unwrap())
}

fn insert(conn: &Connection, id: &str, kind: &str, pinned: i64) {
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, body_inline, tags_json,
            created_at, updated_at, is_pinned_wakeup)
         VALUES (?1, ?1, ?2, 'global', ?1, ?3, '[]', '2026-01-01T00:00:00Z',
            '2026-01-01T00:00:00Z', ?4)",
        params![id, kind, format!("body of {id}"), pinned],
    )
    .unwrap();
}

fn req<'a>(keeper: &'a str, lesser: &'a str) -> MergeRequest<'a> {
    MergeRequest {
        keeper,
        lesser,
        body: None,
        conflict: false,
    }
}

fn merge(
    conn: &Connection,
    dir: &tempfile::TempDir,
    req: MergeRequest,
    actor: &str,
    cap: u32,
) -> Result<serde_json::Value, MergeError> {
    run_merge(conn, dir.path(), &req, actor, cap, Utc::now())
}

fn kind_of(conn: &Connection, id: &str) -> String {
    conn.query_row(
        "SELECT kind FROM mem_items WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )
    .unwrap()
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn active_merge_edges(conn: &Connection, from: &str, rel: &str, to: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM relations
         WHERE from_id = ?1 AND relation_type = ?2 AND to_id = ?3
           AND valid_to IS NULL AND reason = ?4",
        params![from, rel, to, MERGE_RELATION_REASON],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn mem_merge_archives_lesser_with_history_and_relation() {
    let (conn, dir) = open_test_conn();
    insert(&conn, "fact_keeper", "fact", 0);
    insert(&conn, "fact_lesser", "note", 0);

    let out = merge(
        &conn,
        &dir,
        MergeRequest {
            body: Some("merged body"),
            ..req("fact_keeper", "fact_lesser")
        },
        "user",
        10,
    )
    .unwrap();
    assert_eq!(out["merged"], "fact_lesser");
    assert_eq!(out["body_updated"], true);

    let keeper = store::get(&conn, "fact_keeper", dir.path())
        .unwrap()
        .unwrap();
    assert_eq!(keeper.body, "merged body");
    assert_eq!(kind_of(&conn, "fact_lesser"), ARCHIVED_KIND);
    let history = store::get_item_history(&conn, "fact_lesser").unwrap();
    let last = history.last().unwrap();
    assert_eq!(last.change_type, MERGE_CHANGE_TYPE);
    assert_eq!(last.old_kind.as_deref(), Some("note"));
    assert_eq!(last.new_kind.as_deref(), Some(ARCHIVED_KIND));
    assert_eq!(
        active_merge_edges(&conn, "fact_lesser", MERGED_INTO, "fact_keeper"),
        1
    );

    insert(&conn, "fact_conflict", "fact", 0);
    merge(
        &conn,
        &dir,
        MergeRequest {
            conflict: true,
            ..req("fact_keeper", "fact_conflict")
        },
        "user",
        10,
    )
    .unwrap();
    assert_eq!(
        active_merge_edges(&conn, "fact_keeper", SUPERSEDES, "fact_conflict"),
        1
    );
}

#[test]
fn mem_merge_refuses_protected_lesser() {
    let (conn, dir) = open_test_conn();
    insert(&conn, "keeper", "fact", 0);
    insert(&conn, "pinned_fact", "fact", 1);
    for kind in ["skill", "rule", "preference"] {
        insert(&conn, kind, kind, 0);
    }
    insert(&conn, "already", ARCHIVED_KIND, 0);

    for lesser in ["pinned_fact", "skill", "rule", "preference", "already"] {
        let err = merge(
            &conn,
            &dir,
            MergeRequest {
                body: Some("must not land"),
                ..req("keeper", lesser)
            },
            "user",
            10,
        )
        .unwrap_err();
        assert!(matches!(err, MergeError::Refused(_)), "{lesser}: {err:?}");
        assert_eq!(err.status(), StatusCode::CONFLICT);
    }
    assert_eq!(kind_of(&conn, "skill"), "skill");
    assert_eq!(kind_of(&conn, "pinned_fact"), "fact");
    let keeper = store::get(&conn, "keeper", dir.path()).unwrap().unwrap();
    assert_eq!(keeper.body, "body of keeper");
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM mem_item_history"), 0);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM relations"), 0);
}

#[test]
fn mem_merge_never_hard_deletes() {
    let (conn, dir) = open_test_conn();
    insert(&conn, "keeper", "fact", 0);
    insert(&conn, "lesser", "fact", 0);
    let before = count(&conn, "SELECT COUNT(*) FROM mem_items");

    merge(
        &conn,
        &dir,
        MergeRequest {
            body: Some("merged"),
            ..req("keeper", "lesser")
        },
        "user",
        10,
    )
    .unwrap();

    assert_eq!(count(&conn, "SELECT COUNT(*) FROM mem_items"), before);
    let lesser = store::get(&conn, "lesser", dir.path()).unwrap().unwrap();
    assert_eq!(lesser.body, "body of lesser", "the lesser keeps its body");
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM mem_item_history WHERE change_type = 'delete'"
        ),
        0
    );
}

#[test]
fn mem_merge_cap_refuses_past_limit() {
    let (conn, dir) = open_test_conn();
    insert(&conn, "keeper", "fact", 0);
    for id in ["l1", "l2", "l3", "l4"] {
        insert(&conn, id, "fact", 0);
    }
    let dreamer = merge_actor(Some("dreaming:rem"), None);
    assert_eq!(dreamer, DREAMER_MERGE_ACTOR);
    assert_ne!(
        merge_actor(None, Some(DREAMER_MERGE_ACTOR)),
        DREAMER_MERGE_ACTOR,
        "a client label can never claim the dreamer actor"
    );
    assert_eq!(merge_actor(Some("librarian"), None), "worker:librarian");

    merge(&conn, &dir, req("keeper", "l1"), &dreamer, 2).unwrap();
    merge(&conn, &dir, req("keeper", "l2"), &dreamer, 2).unwrap();
    let err = merge(&conn, &dir, req("keeper", "l3"), &dreamer, 2).unwrap_err();
    assert!(matches!(err, MergeError::CapReached(_)), "{err:?}");
    assert_eq!(err.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        kind_of(&conn, "l3"),
        "fact",
        "a refused merge changes nothing"
    );
    assert_eq!(dreamer_merges_today(&conn, Utc::now()).unwrap(), 2);

    merge(&conn, &dir, req("keeper", "l4"), "user", 2)
        .expect("the nightly cap binds only the dreamer");
}

#[test]
fn mem_restore_unarchives_merged_item_and_invalidates_link() {
    let (conn, dir) = open_test_conn();
    insert(&conn, "keeper", "fact", 0);
    insert(&conn, "lesser", "note", 0);
    merge(
        &conn,
        &dir,
        MergeRequest {
            body: Some("merged"),
            ..req("keeper", "lesser")
        },
        DREAMER_MERGE_ACTOR,
        10,
    )
    .unwrap();

    let history = store::get_item_history(&conn, "lesser").unwrap();
    let plan = pick_restore_kind(&history).expect("a merged item restores its kind");
    assert_eq!(plan.kind, "note");
    let out = apply_kind_restore(&conn, dir.path(), "lesser", &plan, None, "restore").unwrap();
    assert_eq!(out["invalidated_relations"], 1);

    assert_eq!(kind_of(&conn, "lesser"), "note");
    assert_eq!(
        active_merge_edges(&conn, "lesser", MERGED_INTO, "keeper"),
        0
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM relations WHERE from_id = 'lesser' AND valid_to IS NOT NULL"
        ),
        1,
        "the merge edge is invalidated, not deleted"
    );
    let after = store::get_item_history(&conn, "lesser").unwrap();
    assert_eq!(after.last().unwrap().change_type, "restore");
    assert_eq!(
        pick_restore_kind(&after),
        None,
        "a restored item is no longer archived"
    );
}

#[test]
fn mem_merge_restore_round_trips_calibration_archive() {
    let (conn, dir) = open_test_conn();
    insert(&conn, "stale_fact", "fact", 0);
    archive_item(
        &conn,
        "stale_fact",
        "fact",
        super::super::calibration::stale::STALE_ACTOR,
        "archive",
        Some("stale"),
    )
    .unwrap();
    let history = store::get_item_history(&conn, "stale_fact").unwrap();
    let plan = pick_restore_kind(&history).unwrap();
    let version = history.last().unwrap().id;
    assert!(
        pick_restore_body(&history, Some(version)).is_err(),
        "an archive row carries no body"
    );

    apply_kind_restore(
        &conn,
        dir.path(),
        "stale_fact",
        &plan,
        Some("restored body"),
        "restore",
    )
    .unwrap();
    assert_eq!(kind_of(&conn, "stale_fact"), "fact");
    let item = store::get(&conn, "stale_fact", dir.path())
        .unwrap()
        .unwrap();
    assert_eq!(item.body, "restored body");
}
