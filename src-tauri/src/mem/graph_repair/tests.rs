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

#[test]
fn find_duplicate_relations_errors_on_empty_group_by() {
    let conn = fresh_conn();
    let err = find_duplicate_relations(&conn, &[], 10).unwrap_err();
    assert!(err.contains("group_by"), "error should mention group_by, got: {}", err);
}

#[test]
fn find_duplicate_relations_returns_empty_when_no_dupes() {
    let conn = fresh_conn();
    let now = "2026-01-01T00:00:00Z";
    insert_relation(&conn, &rel("a","knows","b", now)).unwrap();
    insert_relation(&conn, &rel("c","knows","d", now)).unwrap();
    let groups = find_duplicate_relations(
        &conn,
        &[RelationColumn::FromId, RelationColumn::RelationType, RelationColumn::ToId],
        100,
    ).unwrap();
    assert!(groups.is_empty());
}

#[test]
fn find_duplicate_relations_respects_limit() {
    let conn = fresh_conn();
    insert_relation(&conn, &rel("a","knows","b", "2026-01-01T00:00:00Z")).unwrap();
    insert_relation(&conn, &rel("a","knows","b", "2026-01-02T00:00:00Z")).unwrap();
    insert_relation(&conn, &rel("c","knows","d", "2026-01-01T00:00:00Z")).unwrap();
    insert_relation(&conn, &rel("c","knows","d", "2026-01-02T00:00:00Z")).unwrap();
    insert_relation(&conn, &rel("c","knows","d", "2026-01-03T00:00:00Z")).unwrap();

    let groups = find_duplicate_relations(
        &conn,
        &[RelationColumn::FromId, RelationColumn::RelationType, RelationColumn::ToId],
        1,
    ).unwrap();
    assert_eq!(groups.len(), 1, "limit=1 should yield only top group");
    assert_eq!(groups[0].count, 3, "ORDER BY count DESC should put (c,knows,d) first");
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

    let rowids: Vec<i64> = conn.prepare("SELECT id FROM relations ORDER BY id").unwrap()
        .query_map([], |r| r.get(0)).unwrap()
        .collect::<Result<_, _>>().unwrap();
    assert_eq!(rowids.len(), 2);

    let deleted = delete_relations_by_rowids(&conn, &[rowids[1]], "test").unwrap();
    assert_eq!(deleted, 1);

    let remaining: i64 = conn.query_row("SELECT COUNT(*) FROM relations", [], |r| r.get(0)).unwrap();
    assert_eq!(remaining, 1);

    let (count, actor, details): (i64, String, String) = conn.query_row(
        "SELECT count, actor, details_json FROM graph_purge_log ORDER BY id DESC LIMIT 1",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    ).unwrap();
    assert_eq!(count, 1);
    assert_eq!(actor, "test");
    let parsed: serde_json::Value = serde_json::from_str(&details).unwrap();
    assert!(parsed["payload"].is_array(), "audit must include payload for reversibility");
    assert_eq!(parsed["payload"][0]["properties_json"], "{\"k\":1}");
}

#[test]
fn delete_relations_by_rowids_empty_list_is_noop() {
    let conn = fresh_conn();
    insert_relation(&conn, &rel("a","knows","b","2026-01-01T00:00:00Z")).unwrap();
    let deleted = delete_relations_by_rowids(&conn, &[], "test").unwrap();
    assert_eq!(deleted, 0);

    let remaining: i64 = conn.query_row("SELECT COUNT(*) FROM relations", [], |r| r.get(0)).unwrap();
    assert_eq!(remaining, 1);

    let audits: i64 = conn.query_row("SELECT COUNT(*) FROM graph_purge_log", [], |r| r.get(0)).unwrap();
    assert_eq!(audits, 0, "empty list should not write an audit entry");
}

#[test]
fn delete_relations_by_rowids_nonexistent_rowid_is_audited_as_zero() {
    let conn = fresh_conn();
    let deleted = delete_relations_by_rowids(&conn, &[999_999], "test").unwrap();
    assert_eq!(deleted, 0);

    let (count, details): (i64, String) = conn.query_row(
        "SELECT count, details_json FROM graph_purge_log ORDER BY id DESC LIMIT 1",
        [], |r| Ok((r.get(0)?, r.get(1)?))
    ).unwrap();
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
    let ids: Vec<i64> = conn.prepare("SELECT id FROM relations ORDER BY id").unwrap()
        .query_map([], |r| r.get(0)).unwrap()
        .collect::<Result<_, _>>().unwrap();

    let report = merge_relations(&conn, ids[0], &[ids[1], ids[2]], "test").unwrap();
    assert_eq!(report.merged_into, ids[0]);
    assert_eq!(report.deleted_rowids, vec![ids[1], ids[2]]);
    assert!((report.final_confidence - 0.9).abs() < 1e-6);

    let (conf, props_json): (f64, String) = conn.query_row(
        "SELECT confidence, properties_json FROM relations WHERE id = ?1",
        rusqlite::params![ids[0]], |r| Ok((r.get(0)?, r.get(1)?))
    ).unwrap();
    assert!((conf - 0.9).abs() < 1e-6, "confidence should be max(0.7, 0.9, 0.5) = 0.9");

    let props: serde_json::Value = serde_json::from_str(&props_json).unwrap();
    assert_eq!(props["src"], "x", "kept-side wins on key collision");
    assert_eq!(props["shared"], 1, "kept-side wins on collision");
    assert_eq!(props["extra"], 2, "non-colliding keys are carried over");

    let remaining: i64 = conn.query_row("SELECT COUNT(*) FROM relations", [], |r| r.get(0)).unwrap();
    assert_eq!(remaining, 1);

    let (count, actor, details): (i64, String, String) = conn.query_row(
        "SELECT count, actor, details_json FROM graph_purge_log ORDER BY id DESC LIMIT 1",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    ).unwrap();
    assert_eq!(count, 2);
    assert_eq!(actor, "test");
    let parsed: serde_json::Value = serde_json::from_str(&details).unwrap();
    assert_eq!(parsed["op"], "merge_relations");
    assert_eq!(parsed["kept"], ids[0]);
}

#[test]
fn merge_relations_empty_merge_list_errors() {
    let conn = fresh_conn();
    insert_relation(&conn, &rel("a","knows","b","2026-01-01T00:00:00Z")).unwrap();
    let err = merge_relations(&conn, 1, &[], "test").unwrap_err();
    assert!(err.contains("merge"), "error should mention merge list, got: {}", err);
}

#[test]
fn merge_relations_keep_in_merge_list_errors() {
    let conn = fresh_conn();
    insert_relation(&conn, &rel("a","knows","b","2026-01-01T00:00:00Z")).unwrap();
    insert_relation(&conn, &rel("a","knows","b","2026-01-02T00:00:00Z")).unwrap();
    let err = merge_relations(&conn, 1, &[1, 2], "test").unwrap_err();
    assert!(err.contains("keep"), "error should mention keep, got: {}", err);
}

#[test]
fn merge_relations_missing_rowid_errors() {
    let conn = fresh_conn();
    insert_relation(&conn, &rel("a","knows","b","2026-01-01T00:00:00Z")).unwrap();
    let err = merge_relations(&conn, 1, &[999], "test").unwrap_err();
    assert!(!err.is_empty(), "should error when a rowid doesn't exist");
}

#[test]
fn find_duplicate_entities_by_exact_name() {
    let conn = fresh_conn();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(&conn, &Entity {
        id: "person_andrey".into(), entity_type: "Person".into(),
        scope: "global".into(), properties: serde_json::json!({"name": "Andrey"}),
    }).unwrap();
    insert_entity(&conn, &Entity {
        id: "person_andreyy".into(), entity_type: "Person".into(),
        scope: "global".into(), properties: serde_json::json!({"name": "andrey "}),
    }).unwrap();
    insert_entity(&conn, &Entity {
        id: "person_other".into(), entity_type: "Person".into(),
        scope: "global".into(), properties: serde_json::json!({"name": "Anna"}),
    }).unwrap();

    let groups = find_duplicate_entities(&conn, EntitySimilarityStrategy::ExactName, 100).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].entity_type, "Person");
    assert_eq!(groups[0].member_ids.len(), 2);
    assert!(groups[0].member_ids.contains(&"person_andrey".to_string()));
    assert!(groups[0].member_ids.contains(&"person_andreyy".to_string()));
    assert_eq!(groups[0].reason, "exact_name");
}

#[test]
fn find_duplicate_entities_by_alias_chain() {
    let conn = fresh_conn();
    use crate::mem::graph::{insert_entity, Entity, add_alias};
    insert_entity(&conn, &Entity {
        id: "canonical".into(), entity_type: "Person".into(),
        scope: "global".into(), properties: serde_json::json!({"name": "Andrey"}),
    }).unwrap();
    insert_entity(&conn, &Entity {
        id: "alias1".into(), entity_type: "Person".into(),
        scope: "global".into(), properties: serde_json::json!({"name": "alias1"}),
    }).unwrap();
    insert_entity(&conn, &Entity {
        id: "alias2".into(), entity_type: "Person".into(),
        scope: "global".into(), properties: serde_json::json!({"name": "alias2"}),
    }).unwrap();
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
    insert_entity(&conn, &Entity {
        id: "person_andrey".into(), entity_type: "Person".into(),
        scope: "global".into(), properties: serde_json::json!({"name": "Andrey"}),
    }).unwrap();
    let groups = find_duplicate_entities(&conn, EntitySimilarityStrategy::ExactName, 100).unwrap();
    assert!(groups.is_empty());
}

#[test]
fn augment_entity_patches_properties_and_logs() {
    let conn = fresh_conn();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(&conn, &Entity {
        id: "person_andrey".into(),
        entity_type: "Person".into(),
        scope: "global".into(),
        properties: serde_json::json!({"name": "Andrey"}),
    }).unwrap();

    let mut patch = serde_json::Map::new();
    patch.insert("city".into(), serde_json::json!("Riga"));
    patch.insert("description".into(), serde_json::json!("Senior iOS dev, 10y XP"));

    augment_entity(&conn, "person_andrey", patch, "dreamer").unwrap();

    let props: String = conn.query_row(
        "SELECT properties_json FROM entities WHERE id = 'person_andrey'",
        [], |r| r.get(0)
    ).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&props).unwrap();
    assert_eq!(parsed["name"], "Andrey", "existing non-collision keys preserved");
    assert_eq!(parsed["city"], "Riga", "new keys added");
    assert_eq!(parsed["description"], "Senior iOS dev, 10y XP");

    let (count, actor, details): (i64, String, String) = conn.query_row(
        "SELECT count, actor, details_json FROM graph_purge_log ORDER BY id DESC LIMIT 1",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    ).unwrap();
    assert_eq!(count, 0, "augment is non-destructive");
    assert_eq!(actor, "dreamer");
    let parsed_d: serde_json::Value = serde_json::from_str(&details).unwrap();
    assert_eq!(parsed_d["op"], "augment_entity");
    assert_eq!(parsed_d["entity_id"], "person_andrey");
}

#[test]
fn augment_entity_overwrites_existing_keys() {
    let conn = fresh_conn();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(&conn, &Entity {
        id: "person_andrey".into(),
        entity_type: "Person".into(),
        scope: "global".into(),
        properties: serde_json::json!({"city": "OldCity"}),
    }).unwrap();

    let mut patch = serde_json::Map::new();
    patch.insert("city".into(), serde_json::json!("Riga"));

    augment_entity(&conn, "person_andrey", patch, "dreamer").unwrap();

    let props: String = conn.query_row(
        "SELECT properties_json FROM entities WHERE id = 'person_andrey'",
        [], |r| r.get(0)
    ).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&props).unwrap();
    assert_eq!(parsed["city"], "Riga", "patch overwrites existing key");
}

#[test]
fn augment_entity_missing_entity_errors() {
    let conn = fresh_conn();
    let mut patch = serde_json::Map::new();
    patch.insert("city".into(), serde_json::json!("Riga"));
    let err = augment_entity(&conn, "nonexistent", patch, "dreamer").unwrap_err();
    assert!(err.contains("nonexistent") || err.contains("not found"),
            "error should reference missing entity, got: {}", err);
}
