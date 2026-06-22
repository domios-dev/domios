use super::common::*;
use crate::mem::graph::insert_relation;
use crate::mem::graph_repair::*;

#[test]
fn delete_entity_no_relations_succeeds() {
    let conn = fresh_conn();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(
        &conn,
        &Entity {
            id: "person_orphan".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"name": "Orphan"}),
        },
    )
    .unwrap();

    let report = delete_entity_by_id(&conn, "person_orphan", false, false, "dreamer").unwrap();
    assert_eq!(report.entity_id, "person_orphan");
    assert_eq!(report.cascaded_relations, 0);

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM entities WHERE id = 'person_orphan'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);

    let (count, actor, details): (i64, String, String) = conn
        .query_row(
            "SELECT count, actor, details_json FROM graph_purge_log ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(actor, "dreamer");
    let parsed: serde_json::Value = serde_json::from_str(&details).unwrap();
    assert_eq!(parsed["op"], "delete_entity_by_id");
    assert_eq!(parsed["entity"]["id"], "person_orphan");
    assert_eq!(parsed["force"], false);
}

#[test]
fn delete_entity_with_active_relations_errors_without_force() {
    let conn = fresh_conn();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(
        &conn,
        &Entity {
            id: "person_a".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({}),
        },
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity {
            id: "person_b".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({}),
        },
    )
    .unwrap();
    insert_relation(
        &conn,
        &rel("person_a", "knows", "person_b", "2026-01-01T00:00:00Z"),
    )
    .unwrap();

    let err = delete_entity_by_id(&conn, "person_a", false, false, "dreamer").unwrap_err();
    assert!(
        err.contains("active relations"),
        "error should mention active relations, got: {}",
        err
    );

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM entities WHERE id = 'person_a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "entity should NOT have been deleted");
}

#[test]
fn delete_entity_force_cascades_relations() {
    let conn = fresh_conn();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(
        &conn,
        &Entity {
            id: "person_a".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({}),
        },
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity {
            id: "person_b".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({}),
        },
    )
    .unwrap();
    insert_relation(
        &conn,
        &rel("person_a", "knows", "person_b", "2026-01-01T00:00:00Z"),
    )
    .unwrap();
    insert_relation(
        &conn,
        &rel("person_b", "knows", "person_a", "2026-01-01T00:00:00Z"),
    )
    .unwrap();

    let report = delete_entity_by_id(&conn, "person_a", true, false, "dreamer").unwrap();
    assert_eq!(report.cascaded_relations, 2);

    let ent: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM entities WHERE id = 'person_a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ent, 0);

    let rels: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM relations WHERE valid_to IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rels, 0);

    let details: String = conn
        .query_row(
            "SELECT details_json FROM graph_purge_log ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&details).unwrap();
    let cascaded = parsed["cascaded_relations"].as_array().unwrap();
    assert_eq!(
        cascaded.len(),
        2,
        "audit should include both cascaded relations"
    );
    assert!(parsed["cascaded_relations"][0]["from_id"].is_string());
}

#[test]
fn delete_entity_missing_errors() {
    let conn = fresh_conn();
    let err = delete_entity_by_id(&conn, "nonexistent", false, false, "dreamer").unwrap_err();
    assert!(
        err.contains("nonexistent") || err.contains("not found"),
        "should reference missing entity, got: {}",
        err
    );
}

#[test]
fn delete_entity_cleans_mem_vectors() {
    let conn = fresh_conn();
    // Create a minimal mem_vectors table (real one is vec0 virtual, but for cleanup verification a plain table is enough — same DELETE SQL works).
    conn.execute_batch("CREATE TABLE mem_vectors (mem_id TEXT, embedding BLOB)")
        .unwrap();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(
        &conn,
        &Entity {
            id: "person_x".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({}),
        },
    )
    .unwrap();
    conn.execute(
        "INSERT INTO mem_vectors VALUES ('person_x', X''), ('person_x_chunk_1', X''), ('person_other', X'')",
        [],
    ).unwrap();

    delete_entity_by_id(&conn, "person_x", false, false, "dreamer").unwrap();

    let remaining: i64 = conn.query_row(
        "SELECT COUNT(*) FROM mem_vectors WHERE mem_id = 'person_x' OR mem_id LIKE 'person_x_chunk_%'",
        [], |r| r.get(0)
    ).unwrap();
    assert_eq!(remaining, 0, "vectors for the deleted entity must be gone");

    let untouched: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mem_vectors WHERE mem_id = 'person_other'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(untouched, 1, "unrelated vectors must NOT be deleted");
}

#[test]
fn delete_entity_cleans_entity_aliases() {
    let conn = fresh_conn();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(
        &conn,
        &Entity {
            id: "canonical".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({}),
        },
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity {
            id: "other".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({}),
        },
    )
    .unwrap();
    let now = "2026-01-01T00:00:00Z";
    conn.execute(
        "INSERT INTO entity_aliases (alias_id, canonical_id, created_at) VALUES
            ('alias_a', 'canonical', ?1),
            ('alias_b', 'canonical', ?1),
            ('other_alias', 'other', ?1)",
        rusqlite::params![now],
    )
    .unwrap();

    delete_entity_by_id(&conn, "canonical", false, false, "dreamer").unwrap();

    let touching_canonical: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entity_aliases WHERE alias_id = 'canonical' OR canonical_id = 'canonical'",
        [], |r| r.get(0)
    ).unwrap();
    assert_eq!(
        touching_canonical, 0,
        "alias rows pointing at deleted canonical must be cleaned"
    );

    let untouched: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM entity_aliases WHERE canonical_id = 'other'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(untouched, 1, "unrelated alias rows must stay");
}

// `augment_entity` re-embeds after property UPDATE in production.
// Unit tests skip the embedder; end-to-end check happens in `evals/dreamer-repair`.

#[test]
fn delete_entity_purge_history_cascades_invalidated_relations() {
    let conn = fresh_conn();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(
        &conn,
        &Entity {
            id: "person_x".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({}),
        },
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity {
            id: "person_y".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({}),
        },
    )
    .unwrap();
    let now = "2026-01-01T00:00:00Z";
    let later = "2026-01-02T00:00:00Z";
    conn.execute(
        "INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json, created_at)
         VALUES ('person_x','knows','person_y','global',?1,?2,1.0,'{}',?1),
                ('person_x','old_rel','person_y','global',?1,?2,1.0,'{}',?1)",
        rusqlite::params![now, later],
    ).unwrap();

    let active_before: i64 = conn.query_row(
        "SELECT COUNT(*) FROM relations WHERE valid_to IS NULL AND (from_id='person_x' OR to_id='person_x')",
        [], |r| r.get(0)
    ).unwrap();
    assert_eq!(
        active_before, 0,
        "test setup: only invalidated relations seeded"
    );

    let report = delete_entity_by_id(&conn, "person_x", false, true, "test").unwrap();
    assert_eq!(report.cascaded_relations, 0);
    assert_eq!(report.cascaded_history, 2);

    let remaining_history: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM relations WHERE from_id='person_x' OR to_id='person_x'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        remaining_history, 0,
        "invalidated relations must be purged with purge_history=true"
    );
}

#[test]
fn delete_entity_default_keeps_history() {
    let conn = fresh_conn();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(
        &conn,
        &Entity {
            id: "person_x".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({}),
        },
    )
    .unwrap();
    let now = "2026-01-01T00:00:00Z";
    let later = "2026-01-02T00:00:00Z";
    conn.execute(
        "INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json, created_at)
         VALUES ('person_x','knows','other','global',?1,?2,1.0,'{}',?1)",
        rusqlite::params![now, later],
    ).unwrap();

    let report = delete_entity_by_id(&conn, "person_x", false, false, "test").unwrap();
    assert_eq!(report.cascaded_history, 0);

    let ghosts: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM relations WHERE from_id='person_x'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ghosts, 1, "default: history preserved as ghost rows");
}
