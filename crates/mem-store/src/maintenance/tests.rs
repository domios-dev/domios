use super::*;
use rusqlite::{params, Connection};
fn fixture() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    crate::migrations::run(&conn).unwrap();
    conn
}
fn item(conn: &Connection, id: &str, kind: &str, importance: f64, accesses: i64) {
    conn.execute("INSERT INTO mem_items(id,uid,kind,scope,title,importance,access_count,created_at,updated_at,accessed_at) VALUES(?1,?1,?2,'global',?1,?3,?4,'2000-01-01T00:00:00Z','2000-01-01T00:00:00Z',?5)",params![id,kind,importance,accesses,chrono::Utc::now().to_rfc3339()]).unwrap();
}
#[test]
fn light_promotes_learning_and_keeps_neighbor_authorship() {
    let conn = fixture();
    item(&conn, "learning-owner", "learning", 5., 10);
    item(&conn, "learning-neighbor", "learning", 5., 10);
    conn.execute("UPDATE mem_items SET author_kind='neighbor',author_uid='friend',author_friendship='friendship' WHERE id='learning-neighbor'",[]).unwrap();
    let report = worker_dreaming_light(&conn, "machine").unwrap();
    assert_eq!(report.considered, 2);
    assert_eq!(report.promoted, 1);
    assert_eq!(
        conn.query_row(
            "SELECT kind FROM mem_items WHERE id='learning-owner'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "fact"
    );
    assert_eq!(
        conn.query_row(
            "SELECT kind FROM mem_items WHERE id='learning-neighbor'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "learning"
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM mem_item_history WHERE change_type='promote'",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        1
    );
}
#[test]
fn deep_expires_only_old_low_importance_unaccessed_content() {
    let conn = fixture();
    item(&conn, "old-note", "note", 2., 0);
    item(&conn, "valuable-note", "note", 4., 0);
    item(&conn, "read-note", "note", 2., 1);
    let report = worker_dreaming_deep(&conn).unwrap();
    assert_eq!(report.dropped, 1);
    assert_eq!(
        conn.query_row(
            "SELECT importance FROM mem_items WHERE id='old-note'",
            [],
            |r| r.get::<_, f64>(0)
        )
        .unwrap(),
        0.
    );
    assert_eq!(
        conn.query_row(
            "SELECT importance FROM mem_items WHERE id='valuable-note'",
            [],
            |r| r.get::<_, f64>(0)
        )
        .unwrap(),
        4.
    );
    assert_eq!(
        conn.query_row(
            "SELECT importance FROM mem_items WHERE id='read-note'",
            [],
            |r| r.get::<_, f64>(0)
        )
        .unwrap(),
        2.
    );
}
#[test]
fn normalize_revives_live_endpoints_and_leaves_missing_endpoints_invalid() {
    let conn = fixture();
    for id in ["workspace_alpha", "workspace_beta"] {
        conn.execute("INSERT INTO entities(id,entity_type,scope,created_at,updated_at) VALUES(?1,'workspace','global','2026-01-01','2026-01-01')",[id]).unwrap();
    }
    for target in ["workspace_beta", "workspace_missing"] {
        conn.execute("INSERT INTO relations(from_id,relation_type,to_id,scope,valid_from,valid_to,created_at) VALUES('workspace_alpha','uses',?1,'global','2026-01-01','2026-01-02','2026-01-01')",[target]).unwrap();
    }
    let report = worker_dreaming_normalize(&conn).unwrap();
    assert_eq!(report["broken_relations_revived"], 1);
    assert!(conn
        .query_row(
            "SELECT valid_to IS NULL FROM relations WHERE to_id='workspace_beta'",
            [],
            |r| r.get::<_, bool>(0)
        )
        .unwrap());
    assert!(!conn
        .query_row(
            "SELECT valid_to IS NULL FROM relations WHERE to_id='workspace_missing'",
            [],
            |r| r.get::<_, bool>(0)
        )
        .unwrap());
}
