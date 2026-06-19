use super::common::*;
use crate::mem::graph_repair::*;

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
