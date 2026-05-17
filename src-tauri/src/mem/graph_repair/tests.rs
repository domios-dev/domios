use rusqlite::Connection;
use crate::mem::graph::{insert_relation, Relation};
use crate::mem::store::init_schema;
use super::*;

fn fresh_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_schema(&conn).unwrap();
    conn
}

fn rel(from: &str, rel: &str, to: &str, vf: &str) -> Relation {
    Relation {
        from_id: from.into(),
        relation_type: rel.into(),
        to_id: to.into(),
        scope: "global".into(),
        valid_from: vf.into(),
        valid_to: None,
        confidence: 1.0,
        properties: serde_json::json!({}),
    }
}

#[test]
fn find_duplicate_relations_groups_by_exact_key() {
    let conn = fresh_conn();
    // Three open relations with same (from, rel, to) but distinct valid_from
    // (UNIQUE(from_id, relation_type, to_id, valid_from) — distinct valid_from
    // bypasses the constraint while simulating historical duplicate pollution).
    insert_relation(&conn, &rel("a", "knows", "b", "2026-01-01T00:00:00Z")).unwrap();
    insert_relation(&conn, &rel("a", "knows", "b", "2026-01-02T00:00:00Z")).unwrap();
    insert_relation(&conn, &rel("a", "knows", "b", "2026-01-03T00:00:00Z")).unwrap();
    insert_relation(&conn, &rel("c", "knows", "d", "2026-01-01T00:00:00Z")).unwrap();

    let groups = find_duplicate_relations(
        &conn,
        &[RelationColumn::FromId, RelationColumn::RelationType, RelationColumn::ToId],
        100,
    ).unwrap();

    assert_eq!(groups.len(), 1, "only (a,knows,b) should appear as a duplicate group");
    let g = &groups[0];
    assert_eq!(g.count, 3);
    assert_eq!(g.rowids.len(), 3);
    assert_eq!(g.key["from_id"], "a");
    assert_eq!(g.key["relation_type"], "knows");
    assert_eq!(g.key["to_id"], "b");
}
