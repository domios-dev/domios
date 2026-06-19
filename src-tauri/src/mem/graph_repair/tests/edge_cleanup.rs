use super::common::*;
use crate::mem::graph::insert_relation;
use crate::mem::graph_repair::*;
use rusqlite::Connection;

fn active_rel_types(conn: &Connection, from: &str, to: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(
            "SELECT relation_type FROM relations WHERE from_id=?1 AND to_id=?2 AND valid_to IS NULL ORDER BY relation_type",
        )
        .unwrap();
    stmt.query_map([from, to], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

#[test]
fn collapse_synonym_edges_keeps_one_membership_and_all_distinct() {
    let conn = fresh_conn();
    // Parallel synonyms + a distinct predicate on the same pair.
    insert_relation(
        &conn,
        &rel("topic_x", "about", "machine_y", "2026-01-01T00:00:00Z"),
    )
    .unwrap();
    insert_relation(
        &conn,
        &rel("topic_x", "applies_to", "machine_y", "2026-01-02T00:00:00Z"),
    )
    .unwrap();
    insert_relation(
        &conn,
        &rel("topic_x", "runs_on", "machine_y", "2026-01-03T00:00:00Z"),
    )
    .unwrap();
    // A different pair with a single membership edge — must be left alone.
    insert_relation(&conn, &rel("a", "about", "b", "2026-01-01T00:00:00Z")).unwrap();

    let dry = collapse_synonym_edges(&conn, true, "dreamer").unwrap();
    assert!(dry.dry_run);
    assert_eq!(
        dry.orphan_ids.len(),
        1,
        "one redundant membership edge flagged"
    );

    let rep = collapse_synonym_edges(&conn, false, "dreamer").unwrap();
    assert_eq!(rep.deleted, 1);

    let remaining = active_rel_types(&conn, "topic_x", "machine_y");
    assert_eq!(
        remaining,
        vec!["about".to_string(), "runs_on".to_string()],
        "keep ONE membership (about) + the distinct runs_on; drop applies_to"
    );
    // Single-membership pair untouched.
    assert_eq!(active_rel_types(&conn, "a", "b"), vec!["about".to_string()]);
}

#[test]
fn prune_dream_hub_edges_drops_backlinks_keeps_anchor() {
    let conn = fresh_conn();
    use crate::mem::graph::{insert_entity, Entity};
    for id in ["project_aurora", "project_dreams"] {
        insert_entity(
            &conn,
            &Entity {
                id: id.into(),
                entity_type: "Project".into(),
                scope: "global".into(),
                properties: serde_json::json!({}),
            },
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, body_inline, created_at, updated_at, accessed_at, importance)
         VALUES ('daily_dream_x','daily_dream_x','daily','global','dream','a real body here','2026-01-01','2026-01-01','2026-01-01',0.0)",
        [],
    ).unwrap();
    // Noise backlink into a hub + the legit dreams-anchor link.
    insert_relation(
        &conn,
        &rel(
            "daily_dream_x",
            "analyzes",
            "project_aurora",
            "2026-01-01T00:00:00Z",
        ),
    )
    .unwrap();
    insert_relation(
        &conn,
        &rel(
            "daily_dream_x",
            "applies_to",
            "project_aurora",
            "2026-01-02T00:00:00Z",
        ),
    )
    .unwrap();
    insert_relation(
        &conn,
        &rel(
            "daily_dream_x",
            "belongs_to",
            "project_dreams",
            "2026-01-03T00:00:00Z",
        ),
    )
    .unwrap();

    let rep = prune_dream_hub_edges(&conn, false, "dreamer").unwrap();
    assert_eq!(rep.deleted, 2, "both hub backlinks invalidated");
    assert!(
        active_rel_types(&conn, "daily_dream_x", "project_aurora").is_empty(),
        "no dream backlinks left to the hub"
    );
    assert_eq!(
        active_rel_types(&conn, "daily_dream_x", "project_dreams"),
        vec!["belongs_to".to_string()],
        "the dreams-anchor link survives"
    );
}
