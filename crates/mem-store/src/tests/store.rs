use rusqlite::Connection;

use super::init_v1_22_schema;

#[test]
fn bump_evidence_count_increments_and_idempotent_on_missing() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, tags_json, created_at, updated_at)
         VALUES ('m_a','m_a','note','global','t','[]','2026-01-01','2026-01-01')",
        [],
    )
    .unwrap();
    assert!(crate::graph::bump_evidence_count(&conn, "m_a").unwrap());
    let ec: i64 = conn
        .query_row(
            "SELECT evidence_count FROM mem_items WHERE id = 'm_a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ec, 2, "starts at 1, one bump → 2");
    assert!(
        !crate::graph::bump_evidence_count(&conn, "m_missing").unwrap(),
        "missing row must return Ok(false), not error"
    );
}

#[test]
fn bump_false_fire_count_increments_and_idempotent_on_missing() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, tags_json, created_at, updated_at)
         VALUES ('m_b','m_b','note','global','t','[]','2026-01-01','2026-01-01')",
        [],
    )
    .unwrap();
    assert!(crate::graph::bump_false_fire_count(&conn, "m_b").unwrap());
    let ff: i64 = conn
        .query_row(
            "SELECT false_fire_count FROM mem_items WHERE id = 'm_b'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ff, 1, "starts at 0, one bump → 1");
    assert!(
        !crate::graph::bump_false_fire_count(&conn, "m_missing").unwrap(),
        "missing row must return Ok(false), not error"
    );
}
