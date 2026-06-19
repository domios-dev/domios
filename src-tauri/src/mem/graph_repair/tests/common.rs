use crate::mem::graph::Relation;
use crate::mem::store::init_schema;
use rusqlite::Connection;

// Repair tests include legacy rows without the active-row unique index so dup cleanup can be exercised.
pub(super) fn fresh_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_schema(&conn).unwrap();
    conn.execute_batch("DROP INDEX IF EXISTS idx_relations_active_unique;")
        .unwrap();
    conn
}

pub(super) fn rel(from: &str, rel: &str, to: &str, vf: &str) -> Relation {
    Relation {
        from_id: from.into(),
        relation_type: rel.into(),
        to_id: to.into(),
        scope: "global".into(),
        valid_from: vf.into(),
        valid_to: None,
        confidence: 1.0,
        properties: serde_json::json!({}),
        reason: None,
    }
}
