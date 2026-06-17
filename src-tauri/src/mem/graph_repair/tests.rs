use super::*;
use crate::mem::graph::{insert_relation, Relation};
use crate::mem::store::init_schema;
use rusqlite::Connection;

// Repair tests include legacy rows without the active-row unique index so dup cleanup can be exercised.
fn fresh_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_schema(&conn).unwrap();
    conn.execute_batch("DROP INDEX IF EXISTS idx_relations_active_unique;")
        .unwrap();
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
        reason: None,
    }
}

#[test]
fn find_duplicate_relations_groups_by_exact_key() {
    let conn = fresh_conn();
    insert_relation(&conn, &rel("a", "knows", "b", "2026-01-01T00:00:00Z")).unwrap();
    insert_relation(&conn, &rel("a", "knows", "b", "2026-01-02T00:00:00Z")).unwrap();
    insert_relation(&conn, &rel("a", "knows", "b", "2026-01-03T00:00:00Z")).unwrap();
    insert_relation(&conn, &rel("c", "knows", "d", "2026-01-01T00:00:00Z")).unwrap();

    let groups = find_duplicate_relations(
        &conn,
        &[
            RelationColumn::FromId,
            RelationColumn::RelationType,
            RelationColumn::ToId,
        ],
        100,
    )
    .unwrap();

    assert_eq!(
        groups.len(),
        1,
        "only (a,knows,b) should appear as a duplicate group"
    );
    let g = &groups[0];
    assert_eq!(g.count, 3);
    assert_eq!(g.rowids.len(), 3);
    assert_eq!(g.key["from_id"], "a");
    assert_eq!(g.key["relation_type"], "knows");
    assert_eq!(g.key["to_id"], "b");
}

#[test]
fn find_duplicate_relations_errors_on_empty_group_by() {
    let conn = fresh_conn();
    let err = find_duplicate_relations(&conn, &[], 10).unwrap_err();
    assert!(
        err.contains("group_by"),
        "error should mention group_by, got: {}",
        err
    );
}

#[test]
fn find_duplicate_relations_returns_empty_when_no_dupes() {
    let conn = fresh_conn();
    let now = "2026-01-01T00:00:00Z";
    insert_relation(&conn, &rel("a", "knows", "b", now)).unwrap();
    insert_relation(&conn, &rel("c", "knows", "d", now)).unwrap();
    let groups = find_duplicate_relations(
        &conn,
        &[
            RelationColumn::FromId,
            RelationColumn::RelationType,
            RelationColumn::ToId,
        ],
        100,
    )
    .unwrap();
    assert!(groups.is_empty());
}

#[test]
fn find_duplicate_relations_respects_limit() {
    let conn = fresh_conn();
    insert_relation(&conn, &rel("a", "knows", "b", "2026-01-01T00:00:00Z")).unwrap();
    insert_relation(&conn, &rel("a", "knows", "b", "2026-01-02T00:00:00Z")).unwrap();
    insert_relation(&conn, &rel("c", "knows", "d", "2026-01-01T00:00:00Z")).unwrap();
    insert_relation(&conn, &rel("c", "knows", "d", "2026-01-02T00:00:00Z")).unwrap();
    insert_relation(&conn, &rel("c", "knows", "d", "2026-01-03T00:00:00Z")).unwrap();

    let groups = find_duplicate_relations(
        &conn,
        &[
            RelationColumn::FromId,
            RelationColumn::RelationType,
            RelationColumn::ToId,
        ],
        1,
    )
    .unwrap();
    assert_eq!(groups.len(), 1, "limit=1 should yield only top group");
    assert_eq!(
        groups[0].count, 3,
        "ORDER BY count DESC should put (c,knows,d) first"
    );
}

#[test]
fn delete_relations_by_rowids_audits_payload() {
    let conn = fresh_conn();
    let now = "2026-01-01T00:00:00Z";
    conn.execute(
        "INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json, created_at)
         VALUES ('a','knows','b','global',?1,NULL,1.0,'{}',?1),
                ('a','knows','b','global',?2,NULL,0.9,'{\"k\":1}',?1)",
        rusqlite::params![now, "2026-01-02T00:00:00Z"],
    ).unwrap();

    let rowids: Vec<i64> = conn
        .prepare("SELECT id FROM relations ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rowids.len(), 2);

    let deleted = delete_relations_by_rowids(&conn, &[rowids[1]], "test").unwrap();
    assert_eq!(deleted, 1);

    let remaining: i64 = conn
        .query_row("SELECT COUNT(*) FROM relations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, 1);

    let (count, actor, details): (i64, String, String) = conn
        .query_row(
            "SELECT count, actor, details_json FROM graph_purge_log ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(actor, "test");
    let parsed: serde_json::Value = serde_json::from_str(&details).unwrap();
    assert!(
        parsed["payload"].is_array(),
        "audit must include payload for reversibility"
    );
    assert_eq!(parsed["payload"][0]["properties_json"], "{\"k\":1}");
}

#[test]
fn delete_relations_by_rowids_empty_list_is_noop() {
    let conn = fresh_conn();
    insert_relation(&conn, &rel("a", "knows", "b", "2026-01-01T00:00:00Z")).unwrap();
    let deleted = delete_relations_by_rowids(&conn, &[], "test").unwrap();
    assert_eq!(deleted, 0);

    let remaining: i64 = conn
        .query_row("SELECT COUNT(*) FROM relations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, 1);

    let audits: i64 = conn
        .query_row("SELECT COUNT(*) FROM graph_purge_log", [], |r| r.get(0))
        .unwrap();
    assert_eq!(audits, 0, "empty list should not write an audit entry");
}

#[test]
fn delete_relations_by_rowids_nonexistent_rowid_is_audited_as_zero() {
    let conn = fresh_conn();
    let deleted = delete_relations_by_rowids(&conn, &[999_999], "test").unwrap();
    assert_eq!(deleted, 0);

    let (count, details): (i64, String) = conn
        .query_row(
            "SELECT count, details_json FROM graph_purge_log ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(count, 0);
    let parsed: serde_json::Value = serde_json::from_str(&details).unwrap();
    assert_eq!(parsed["payload"].as_array().unwrap().len(), 0);
}

#[test]
fn merge_relations_carries_over_max_confidence_and_merged_properties() {
    let conn = fresh_conn();
    // UNIQUE(from,rel,to,valid_from) means same (a,knows,b) needs different valid_from values
    conn.execute(
        "INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json, created_at)
         VALUES ('a','knows','b','global','2026-01-01T00:00:00Z',NULL,0.7,'{\"src\":\"x\",\"shared\":1}','2026-01-01T00:00:00Z'),
                ('a','knows','b','global','2026-01-02T00:00:00Z',NULL,0.9,'{\"src\":\"y\",\"extra\":2}','2026-01-02T00:00:00Z'),
                ('a','knows','b','global','2026-01-03T00:00:00Z',NULL,0.5,'{\"shared\":99}','2026-01-03T00:00:00Z')",
        [],
    ).unwrap();
    let ids: Vec<i64> = conn
        .prepare("SELECT id FROM relations ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    let report = merge_relations(&conn, ids[0], &[ids[1], ids[2]], "test").unwrap();
    assert_eq!(report.merged_into, ids[0]);
    assert_eq!(report.deleted_rowids, vec![ids[1], ids[2]]);
    assert!((report.final_confidence - 0.9).abs() < 1e-6);

    let (conf, props_json): (f64, String) = conn
        .query_row(
            "SELECT confidence, properties_json FROM relations WHERE id = ?1",
            rusqlite::params![ids[0]],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(
        (conf - 0.9).abs() < 1e-6,
        "confidence should be max(0.7, 0.9, 0.5) = 0.9"
    );

    let props: serde_json::Value = serde_json::from_str(&props_json).unwrap();
    assert_eq!(props["src"], "x", "kept-side wins on key collision");
    assert_eq!(props["shared"], 1, "kept-side wins on collision");
    assert_eq!(props["extra"], 2, "non-colliding keys are carried over");

    let remaining: i64 = conn
        .query_row("SELECT COUNT(*) FROM relations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, 1);

    let (count, actor, details): (i64, String, String) = conn
        .query_row(
            "SELECT count, actor, details_json FROM graph_purge_log ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(count, 2);
    assert_eq!(actor, "test");
    let parsed: serde_json::Value = serde_json::from_str(&details).unwrap();
    assert_eq!(parsed["op"], "merge_relations");
    assert_eq!(parsed["kept"], ids[0]);
}

#[test]
fn merge_relations_empty_merge_list_errors() {
    let conn = fresh_conn();
    insert_relation(&conn, &rel("a", "knows", "b", "2026-01-01T00:00:00Z")).unwrap();
    let err = merge_relations(&conn, 1, &[], "test").unwrap_err();
    assert!(
        err.contains("merge"),
        "error should mention merge list, got: {}",
        err
    );
}

#[test]
fn merge_relations_keep_in_merge_list_errors() {
    let conn = fresh_conn();
    insert_relation(&conn, &rel("a", "knows", "b", "2026-01-01T00:00:00Z")).unwrap();
    insert_relation(&conn, &rel("a", "knows", "b", "2026-01-02T00:00:00Z")).unwrap();
    let err = merge_relations(&conn, 1, &[1, 2], "test").unwrap_err();
    assert!(
        err.contains("keep"),
        "error should mention keep, got: {}",
        err
    );
}

#[test]
fn merge_relations_missing_rowid_errors() {
    let conn = fresh_conn();
    insert_relation(&conn, &rel("a", "knows", "b", "2026-01-01T00:00:00Z")).unwrap();
    let err = merge_relations(&conn, 1, &[999], "test").unwrap_err();
    assert!(!err.is_empty(), "should error when a rowid doesn't exist");
}

#[test]
fn find_duplicate_entities_by_exact_name() {
    let conn = fresh_conn();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(
        &conn,
        &Entity {
            id: "person_robert".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"name": "Robert"}),
        },
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity {
            id: "person_roberty".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"name": "robert "}),
        },
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity {
            id: "person_other".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"name": "Susan"}),
        },
    )
    .unwrap();

    let groups = find_duplicate_entities(&conn, EntitySimilarityStrategy::ExactName, 100).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].entity_type, "Person");
    assert_eq!(groups[0].member_ids.len(), 2);
    assert!(groups[0].member_ids.contains(&"person_robert".to_string()));
    assert!(groups[0].member_ids.contains(&"person_roberty".to_string()));
    assert_eq!(groups[0].reason, "exact_name");
}

#[test]
fn find_duplicate_entities_by_alias_chain() {
    let conn = fresh_conn();
    use crate::mem::graph::{add_alias, insert_entity, Entity};
    insert_entity(
        &conn,
        &Entity {
            id: "canonical".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"name": "Canonical"}),
        },
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity {
            id: "alias1".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"name": "alias1"}),
        },
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity {
            id: "alias2".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"name": "alias2"}),
        },
    )
    .unwrap();
    add_alias(&conn, "alias1", "canonical").unwrap();
    add_alias(&conn, "alias2", "canonical").unwrap();

    let groups = find_duplicate_entities(&conn, EntitySimilarityStrategy::AliasChain, 100).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].canonical_hint, "canonical");
    assert_eq!(groups[0].member_ids.len(), 3, "canonical + 2 aliases");
    assert!(groups[0].member_ids.contains(&"canonical".to_string()));
}

#[test]
fn find_duplicate_entities_empty_when_no_duplicates() {
    let conn = fresh_conn();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(
        &conn,
        &Entity {
            id: "person_robert".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"name": "Robert"}),
        },
    )
    .unwrap();
    let groups = find_duplicate_entities(&conn, EntitySimilarityStrategy::ExactName, 100).unwrap();
    assert!(groups.is_empty());
}

#[test]
fn augment_entity_patches_properties_and_logs() {
    let conn = fresh_conn();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(
        &conn,
        &Entity {
            id: "person_robert".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"name": "Robert"}),
        },
    )
    .unwrap();

    let mut patch = serde_json::Map::new();
    patch.insert("city".into(), serde_json::json!("Berlin"));
    patch.insert(
        "description".into(),
        serde_json::json!("Backend engineer, 10y XP"),
    );

    augment_entity(&conn, "person_robert", patch, "dreamer").unwrap();

    let props: String = conn
        .query_row(
            "SELECT properties_json FROM entities WHERE id = 'person_robert'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&props).unwrap();
    assert_eq!(
        parsed["name"], "Robert",
        "existing non-collision keys preserved"
    );
    assert_eq!(parsed["city"], "Berlin", "new keys added");
    assert_eq!(parsed["description"], "Backend engineer, 10y XP");

    let (count, actor, details): (i64, String, String) = conn
        .query_row(
            "SELECT count, actor, details_json FROM graph_purge_log ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(count, 0, "augment is non-destructive");
    assert_eq!(actor, "dreamer");
    let parsed_d: serde_json::Value = serde_json::from_str(&details).unwrap();
    assert_eq!(parsed_d["op"], "augment_entity");
    assert_eq!(parsed_d["entity_id"], "person_robert");
}

#[test]
fn augment_entity_overwrites_existing_keys() {
    let conn = fresh_conn();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(
        &conn,
        &Entity {
            id: "person_robert".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"city": "OldCity"}),
        },
    )
    .unwrap();

    let mut patch = serde_json::Map::new();
    patch.insert("city".into(), serde_json::json!("Berlin"));

    augment_entity(&conn, "person_robert", patch, "dreamer").unwrap();

    let props: String = conn
        .query_row(
            "SELECT properties_json FROM entities WHERE id = 'person_robert'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&props).unwrap();
    assert_eq!(parsed["city"], "Berlin", "patch overwrites existing key");
}

#[test]
fn augment_entity_missing_entity_errors() {
    let conn = fresh_conn();
    let mut patch = serde_json::Map::new();
    patch.insert("city".into(), serde_json::json!("Berlin"));
    let err = augment_entity(&conn, "nonexistent", patch, "dreamer").unwrap_err();
    assert!(
        err.contains("nonexistent") || err.contains("not found"),
        "error should reference missing entity, got: {}",
        err
    );
}

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

#[test]
fn repair_orphan_vectors_finds_and_deletes() {
    let conn = fresh_conn();
    conn.execute_batch("CREATE TABLE mem_vectors (mem_id TEXT, embedding BLOB)")
        .unwrap();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(
        &conn,
        &Entity {
            id: "person_real".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({}),
        },
    )
    .unwrap();
    conn.execute(
        "INSERT INTO mem_vectors VALUES
            ('person_real', X''),
            ('person_real_chunk_1', X''),
            ('person_ghost', X''),
            ('person_ghost_chunk_2', X'')",
        [],
    )
    .unwrap();

    let dry = repair_orphan_vectors(&conn, true, "test").unwrap();
    assert!(dry.dry_run);
    assert_eq!(dry.deleted, 0);
    assert_eq!(dry.orphan_ids.len(), 2);
    assert!(dry.orphan_ids.contains(&"person_ghost".to_string()));

    let report = repair_orphan_vectors(&conn, false, "test").unwrap();
    assert_eq!(report.deleted, 2);
    assert_eq!(report.orphan_ids.len(), 2);

    let remaining: i64 = conn
        .query_row("SELECT COUNT(*) FROM mem_vectors", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, 2, "only person_real's vectors should remain");

    let (count, actor): (i64, String) = conn
        .query_row(
            "SELECT count, actor FROM graph_purge_log ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(count, 2);
    assert_eq!(actor, "test");
}

#[test]
fn repair_orphan_vectors_no_orphans_is_noop() {
    let conn = fresh_conn();
    conn.execute_batch("CREATE TABLE mem_vectors (mem_id TEXT, embedding BLOB)")
        .unwrap();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(
        &conn,
        &Entity {
            id: "person_real".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({}),
        },
    )
    .unwrap();
    conn.execute("INSERT INTO mem_vectors VALUES ('person_real', X'')", [])
        .unwrap();

    let report = repair_orphan_vectors(&conn, false, "test").unwrap();
    assert_eq!(report.deleted, 0);
    assert!(report.orphan_ids.is_empty());

    let audits: i64 = conn
        .query_row("SELECT COUNT(*) FROM graph_purge_log", [], |r| r.get(0))
        .unwrap();
    assert_eq!(audits, 0, "no orphans → no audit row");
}

fn insert_item_inline(conn: &Connection, id: &str, body: &str) {
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, body_inline, created_at, updated_at, accessed_at, importance)
         VALUES (?1, ?1, 'note', 'global', 'title', ?2, '2026-01-01', '2026-01-01', '2026-01-01', 0.0)",
        rusqlite::params![id, body],
    )
    .unwrap();
}

#[test]
fn prune_thin_vectors_deletes_only_thin_keeps_thick() {
    let conn = fresh_conn();
    conn.execute_batch("CREATE TABLE mem_vectors (mem_id TEXT, embedding BLOB)")
        .unwrap();

    // Thick: a real, substantive body — its vector is GOOD and must survive.
    insert_item_inline(
        &conn,
        "fact_thick",
        "A genuinely meaningful body with real content worth searching.",
    );
    // Thin: empty and title-only-ish bodies — their vectors are noise.
    insert_item_inline(&conn, "fact_empty", "");
    insert_item_inline(&conn, "fact_short", "hi");
    // Blob-backed: large body lives on disk (body_inline NULL) — never thin.
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, body_blob_path, created_at, updated_at, accessed_at, importance)
         VALUES ('fact_blob','fact_blob','note','global','t','sha256_x.txt','2026-01-01','2026-01-01','2026-01-01',0.0)",
        [],
    ).unwrap();

    conn.execute(
        "INSERT INTO mem_vectors VALUES
            ('fact_thick', X''),
            ('fact_thick_chunk_1', X''),
            ('fact_empty', X''),
            ('fact_short', X''),
            ('fact_blob', X'')",
        [],
    )
    .unwrap();

    let dry = prune_thin_vectors(&conn, true, "dreamer").unwrap();
    assert!(dry.dry_run);
    assert_eq!(dry.deleted, 0);
    // Only the two thin items (+none of thick/blob) are flagged.
    assert_eq!(dry.orphan_ids.len(), 2, "got {:?}", dry.orphan_ids);
    assert!(dry.orphan_ids.contains(&"fact_empty".to_string()));
    assert!(dry.orphan_ids.contains(&"fact_short".to_string()));
    assert!(
        !dry.orphan_ids.contains(&"fact_thick".to_string()),
        "thick (good) vector must never be flagged"
    );
    assert!(!dry.orphan_ids.contains(&"fact_blob".to_string()));

    let report = prune_thin_vectors(&conn, false, "dreamer").unwrap();
    assert_eq!(report.deleted, 2);

    let surviving: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT mem_id FROM mem_vectors ORDER BY mem_id")
            .unwrap();
        let rows: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        rows
    };
    assert_eq!(
        surviving,
        vec![
            "fact_blob".to_string(),
            "fact_thick".to_string(),
            "fact_thick_chunk_1".to_string()
        ],
        "thick body, its chunk, and the blob-backed vector must all survive"
    );

    // Items themselves are never deleted — only the vector rows.
    let item_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM mem_items", [], |r| r.get(0))
        .unwrap();
    assert_eq!(item_count, 4, "no mem_item deleted by vector pruning");

    let (count, actor): (i64, String) = conn
        .query_row(
            "SELECT count, actor FROM graph_purge_log ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(count, 2);
    assert_eq!(actor, "dreamer");
}

#[test]
fn prune_thin_vectors_noop_when_all_thick() {
    let conn = fresh_conn();
    conn.execute_batch("CREATE TABLE mem_vectors (mem_id TEXT, embedding BLOB)")
        .unwrap();
    insert_item_inline(
        &conn,
        "fact_a",
        "Plenty of real searchable content lives in this body.",
    );
    conn.execute("INSERT INTO mem_vectors VALUES ('fact_a', X'')", [])
        .unwrap();

    let report = prune_thin_vectors(&conn, false, "dreamer").unwrap();
    assert_eq!(report.deleted, 0);
    assert!(report.orphan_ids.is_empty());
    let remaining: i64 = conn
        .query_row("SELECT COUNT(*) FROM mem_vectors", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, 1, "good vector untouched");
}

#[test]
fn repair_orphan_blobs_finds_unreferenced_files() {
    let conn = fresh_conn();
    let tmp = tempfile::tempdir().unwrap();
    let blobs_dir = tmp.path();

    // Seed the dir with three CAS-named files and two random ones.
    std::fs::write(blobs_dir.join("sha256_referenced.txt"), "real").unwrap();
    std::fs::write(blobs_dir.join("sha256_orphan_a.txt"), "ghost1").unwrap();
    std::fs::write(blobs_dir.join("sha256_orphan_b.txt"), "ghost2").unwrap();
    std::fs::write(blobs_dir.join("other.txt"), "not a blob").unwrap();

    // Tell the DB about the referenced one.
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, body_blob_path, created_at, updated_at, accessed_at, importance)
         VALUES ('m1','u1','note','global','t','sha256_referenced.txt','2026-01-01','2026-01-01','2026-01-01',0.0)",
        [],
    ).unwrap();

    let dry = repair_orphan_blobs(&conn, blobs_dir, true, "test").unwrap();
    assert!(dry.dry_run);
    assert_eq!(dry.deleted, 0);
    assert_eq!(dry.orphan_files.len(), 2);

    let report = repair_orphan_blobs(&conn, blobs_dir, false, "test").unwrap();
    assert_eq!(report.deleted, 2);
    assert!(
        blobs_dir.join("sha256_referenced.txt").exists(),
        "referenced blob stays"
    );
    assert!(
        !blobs_dir.join("sha256_orphan_a.txt").exists(),
        "orphan deleted"
    );
    assert!(
        !blobs_dir.join("sha256_orphan_b.txt").exists(),
        "orphan deleted"
    );
    assert!(
        blobs_dir.join("other.txt").exists(),
        "non-CAS file untouched"
    );
}

#[test]
fn find_duplicate_entities_vector_threshold_validates() {
    let conn = fresh_conn();
    let err = find_duplicate_entities(
        &conn,
        EntitySimilarityStrategy::Vector {
            threshold: 1.5,
            entity_type: None,
        },
        10,
    )
    .unwrap_err();
    assert!(
        err.contains("threshold"),
        "out-of-range threshold should error, got: {}",
        err
    );
}

#[test]
fn find_duplicate_entities_vector_empty_when_no_vectors() {
    // Without mem_vectors table loaded, vector strategy is a quiet no-op.
    let conn = fresh_conn();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(
        &conn,
        &Entity {
            id: "p_a".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"name": "A"}),
        },
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity {
            id: "p_b".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"name": "A"}),
        },
    )
    .unwrap();

    let groups = find_duplicate_entities(
        &conn,
        EntitySimilarityStrategy::Vector {
            threshold: 0.85,
            entity_type: None,
        },
        10,
    )
    .unwrap();
    assert!(groups.is_empty(), "no mem_vectors → no candidates surfaced");
}
