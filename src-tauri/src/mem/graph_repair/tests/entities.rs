use super::common::*;
use crate::mem::graph_repair::*;

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
