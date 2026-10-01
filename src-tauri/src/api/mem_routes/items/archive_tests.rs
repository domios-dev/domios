use super::archive_mem_item_conn;
use crate::api::mem_routes::mem_merge::{apply_kind_restore, pick_restore_kind};
use crate::mem::store;
use axum::http::StatusCode;
use rusqlite::{params, Connection};
use tempfile::TempDir;

fn database() -> (Connection, TempDir) {
    let conn = Connection::open_in_memory().unwrap();
    store::init_schema(&conn).unwrap();
    (conn, tempfile::tempdir().unwrap())
}

fn insert(conn: &Connection, id: &str, kind: &str, pinned: bool) {
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, body_inline, tags_json,
            created_at, updated_at, is_pinned_wakeup)
         VALUES (?1, ?1, ?2, 'global', ?1, 'body', '[]', '2026-01-01T00:00:00Z',
            '2026-01-01T00:00:00Z', ?3)",
        params![id, kind, i64::from(pinned)],
    )
    .unwrap();
}

fn kind(conn: &Connection, id: &str) -> String {
    conn.query_row("SELECT kind FROM mem_items WHERE id = ?1", [id], |row| {
        row.get(0)
    })
    .unwrap()
}

#[test]
fn mem_archive_is_reversible_via_restore() {
    let (conn, blobs) = database();
    insert(&conn, "fact_archive", "fact", false);

    let output = archive_mem_item_conn(&conn, "fact_archive", "obsolete stub", "test").unwrap();
    assert_eq!(output["archived"], "fact_archive");
    assert_eq!(kind(&conn, "fact_archive"), "archived");
    let history = store::get_item_history(&conn, "fact_archive").unwrap();
    assert_eq!(history.last().unwrap().change_type, "archive");

    let restore = pick_restore_kind(&history).unwrap();
    apply_kind_restore(
        &conn,
        blobs.path(),
        "fact_archive",
        &restore,
        None,
        "restore",
    )
    .unwrap();
    assert_eq!(kind(&conn, "fact_archive"), "fact");
}

#[test]
fn mem_archive_refuses_protected_kinds() {
    let (conn, _) = database();
    for protected in ["skill", "rule", "preference"] {
        insert(&conn, protected, protected, false);
        assert_eq!(
            archive_mem_item_conn(&conn, protected, "test", "test")
                .unwrap_err()
                .0,
            StatusCode::CONFLICT,
        );
        assert_eq!(kind(&conn, protected), protected);
    }
    insert(&conn, "pinned_fact", "fact", true);
    assert_eq!(
        archive_mem_item_conn(&conn, "pinned_fact", "test", "test")
            .unwrap_err()
            .0,
        StatusCode::CONFLICT,
    );
    assert_eq!(kind(&conn, "pinned_fact"), "fact");
}
