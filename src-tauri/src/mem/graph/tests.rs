use super::*;
use crate::mem::store::init_schema;

fn fresh() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_schema(&conn).unwrap();
    conn
}

#[test]
fn entity_roundtrip() {
    let conn = fresh();
    let e = Entity {
        id: "person_kai".into(),
        entity_type: "Person".into(),
        scope: "global".into(),
        properties: serde_json::json!({"name": "Kai"}),
    };
    insert_entity(&conn, &e).unwrap();
    // Idempotent on second insert
    insert_entity(&conn, &e).unwrap();
}

#[test]
fn relation_with_as_of_filter() {
    let conn = fresh();
    insert_relation(
        &conn,
        &Relation {
            from_id: "person_kai".into(),
            relation_type: "works_on".into(),
            to_id: "project_a".into(),
            scope: "global".into(),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: Some("2026-03-01T00:00:00Z".into()),
            confidence: 1.0,
            properties: serde_json::json!({}),
            reason: None,
        },
    )
    .unwrap();
    insert_relation(
        &conn,
        &Relation {
            from_id: "person_kai".into(),
            relation_type: "works_on".into(),
            to_id: "project_b".into(),
            scope: "global".into(),
            valid_from: "2026-03-01T00:00:00Z".into(),
            valid_to: None,
            confidence: 1.0,
            properties: serde_json::json!({}),
            reason: None,
        },
    )
    .unwrap();

    // As of 2026-02-01: should see project_a only
    let r = related(
        &conn,
        "person_kai",
        None,
        Some("2026-02-01T00:00:00Z"),
        false,
    )
    .unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].to_id, "project_a");

    // As of 2026-04-01: should see project_b only
    let r = related(
        &conn,
        "person_kai",
        None,
        Some("2026-04-01T00:00:00Z"),
        false,
    )
    .unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].to_id, "project_b");

    // Default (active-only): only the open relation (project_b).
    let r = related(&conn, "person_kai", None, None, false).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].to_id, "project_b");

    // include_invalidated=true returns full history.
    let r = related(&conn, "person_kai", None, None, true).unwrap();
    assert_eq!(r.len(), 2);
}

#[test]
fn related_is_bidirectional() {
    let conn = fresh();
    insert_relation(
        &conn,
        &Relation {
            from_id: "note_a".into(),
            relation_type: "mentions".into(),
            to_id: "person_alex".into(),
            scope: "global".into(),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            confidence: 1.0,
            properties: serde_json::json!({}),
            reason: None,
        },
    )
    .unwrap();

    // Query target entity — should find the incoming relation
    let r = related(&conn, "person_alex", None, None, false).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].from_id, "note_a");
    assert_eq!(r[0].to_id, "person_alex");

    // Query source — should also find it
    let r = related(&conn, "note_a", None, None, false).unwrap();
    assert_eq!(r.len(), 1);
}

#[test]
fn unique_constraint_dedupes() {
    let conn = fresh();
    let r = Relation {
        from_id: "a".into(),
        relation_type: "rel".into(),
        to_id: "b".into(),
        scope: "global".into(),
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
        confidence: 1.0,
        properties: serde_json::json!({}),
        reason: None,
    };
    insert_relation(&conn, &r).unwrap();
    insert_relation(&conn, &r).unwrap();
    let all = related(&conn, "a", None, None, false).unwrap();
    assert_eq!(all.len(), 1);
}

fn pair_entities(conn: &Connection) {
    insert_entity(
        conn,
        &Entity {
            id: "e_a".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::Value::Null,
        },
    )
    .unwrap();
    insert_entity(
        conn,
        &Entity {
            id: "e_b".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::Value::Null,
        },
    )
    .unwrap();
}

fn active_rel() -> Relation {
    Relation {
        from_id: "e_a".into(),
        relation_type: "knows".into(),
        to_id: "e_b".into(),
        scope: "global".into(),
        valid_from: "2026-01-01T00:00:00.000000+00:00".into(),
        valid_to: None,
        confidence: 1.0,
        properties: serde_json::Value::Null,
        reason: None,
    }
}

fn dead_rel() -> Relation {
    Relation {
        from_id: "e_a".into(),
        relation_type: "loved".into(),
        to_id: "e_b".into(),
        scope: "global".into(),
        valid_from: "2024-01-01T00:00:00.000000+00:00".into(),
        valid_to: Some("2025-06-01T00:00:00.000000+00:00".into()),
        confidence: 1.0,
        properties: serde_json::Value::Null,
        reason: None,
    }
}

#[test]
fn related_excludes_invalidated_by_default() {
    let conn = fresh();
    pair_entities(&conn);
    insert_relation(&conn, &active_rel()).unwrap();
    insert_relation(&conn, &dead_rel()).unwrap();

    let rs = related(&conn, "e_a", None, None, false).unwrap();
    assert_eq!(rs.len(), 1, "exactly one active relation");
    assert_eq!(rs[0].valid_to, None);
}

#[test]
fn related_include_invalidated_returns_both() {
    let conn = fresh();
    pair_entities(&conn);
    insert_relation(&conn, &active_rel()).unwrap();
    insert_relation(&conn, &dead_rel()).unwrap();

    let rs = related(&conn, "e_a", None, None, true).unwrap();
    assert_eq!(rs.len(), 2);
}

#[test]
fn traverse_excludes_invalidated_by_default() {
    let conn = fresh();
    pair_entities(&conn);
    insert_relation(&conn, &active_rel()).unwrap();
    insert_relation(&conn, &dead_rel()).unwrap();

    let res = traverse(&conn, "e_a", 2, 100, None, false).unwrap();
    let kinds: std::collections::HashSet<&str> =
        res.edges.iter().map(|r| r.relation_type.as_str()).collect();
    assert_eq!(
        kinds,
        std::collections::HashSet::from(["knows"]),
        "only the active edge is followed; dead_rel must not appear"
    );
}

#[test]
fn shortest_path_skips_invalidated_edges() {
    let conn = fresh();
    // A -> B (dead) and A -> C -> B (alive): path must go through C
    for id in ["e_a", "e_b", "e_c"] {
        insert_entity(
            &conn,
            &Entity {
                id: id.into(),
                entity_type: "X".into(),
                scope: "global".into(),
                properties: serde_json::Value::Null,
            },
        )
        .unwrap();
    }
    insert_relation(
        &conn,
        &Relation {
            from_id: "e_a".into(),
            relation_type: "direct".into(),
            to_id: "e_b".into(),
            scope: "global".into(),
            valid_from: "2024-01-01T00:00:00.000000+00:00".into(),
            valid_to: Some("2025-01-01T00:00:00.000000+00:00".into()),
            confidence: 1.0,
            properties: serde_json::Value::Null,
            reason: None,
        },
    )
    .unwrap();
    insert_relation(
        &conn,
        &Relation {
            from_id: "e_a".into(),
            relation_type: "via".into(),
            to_id: "e_c".into(),
            scope: "global".into(),
            valid_from: "2026-01-01T00:00:00.000000+00:00".into(),
            valid_to: None,
            confidence: 1.0,
            properties: serde_json::Value::Null,
            reason: None,
        },
    )
    .unwrap();
    insert_relation(
        &conn,
        &Relation {
            from_id: "e_c".into(),
            relation_type: "via".into(),
            to_id: "e_b".into(),
            scope: "global".into(),
            valid_from: "2026-01-01T00:00:00.000000+00:00".into(),
            valid_to: None,
            confidence: 1.0,
            properties: serde_json::Value::Null,
            reason: None,
        },
    )
    .unwrap();

    let path = shortest_path(&conn, "e_a", "e_b", 4, false)
        .unwrap()
        .unwrap();
    assert_eq!(path.len(), 2, "two-hop path through e_c");
    assert!(path.iter().all(|r| r.relation_type == "via"));
}

#[test]
fn tunnels_excludes_entities_only_on_invalidated_edges() {
    let conn = fresh();
    let now = chrono::Utc::now().to_rfc3339();
    // Two mem_items with distinct areas + an entity
    for (id, area) in [("m_a", "alpha"), ("m_b", "beta")] {
        conn.execute(
                "INSERT INTO mem_items (id, uid, kind, scope, title, tags_json, area, created_at, updated_at)
                 VALUES (?1, ?1, 'note', 'global', ?1, '[]', ?2, ?3, ?3)",
                params![id, area, &now],
            ).unwrap();
    }
    insert_entity(
        &conn,
        &Entity {
            id: "person_only_dead".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::Value::Null,
        },
    )
    .unwrap();
    // ONLY invalidated edges connect this entity to the two areas
    for src in ["m_a", "m_b"] {
        insert_relation(
            &conn,
            &Relation {
                from_id: src.into(),
                relation_type: "mentions".into(),
                to_id: "person_only_dead".into(),
                scope: "global".into(),
                valid_from: "2024-01-01T00:00:00.000000+00:00".into(),
                valid_to: Some("2025-06-01T00:00:00.000000+00:00".into()),
                confidence: 1.0,
                properties: serde_json::Value::Null,
                reason: None,
            },
        )
        .unwrap();
    }

    let tnls = tunnels(&conn, false).unwrap();
    let ids: Vec<String> = tnls
        .iter()
        .filter_map(|v| v.get("id").and_then(|s| s.as_str()).map(String::from))
        .collect();
    assert!(
        !ids.contains(&"person_only_dead".to_string()),
        "entity reachable only via invalidated relations must NOT appear in tunnels"
    );
}

#[test]
fn timeline_excludes_invalidated_by_default() {
    let conn = fresh();
    insert_entity(
        &conn,
        &Entity {
            id: "e_a".into(),
            entity_type: "X".into(),
            scope: "global".into(),
            properties: serde_json::Value::Null,
        },
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity {
            id: "e_b".into(),
            entity_type: "X".into(),
            scope: "global".into(),
            properties: serde_json::Value::Null,
        },
    )
    .unwrap();
    insert_relation(
        &conn,
        &Relation {
            from_id: "e_a".into(),
            relation_type: "active".into(),
            to_id: "e_b".into(),
            scope: "global".into(),
            valid_from: "2026-01-01T00:00:00.000000+00:00".into(),
            valid_to: None,
            confidence: 1.0,
            properties: serde_json::Value::Null,
            reason: None,
        },
    )
    .unwrap();
    insert_relation(
        &conn,
        &Relation {
            from_id: "e_a".into(),
            relation_type: "dead".into(),
            to_id: "e_b".into(),
            scope: "global".into(),
            valid_from: "2025-01-01T00:00:00.000000+00:00".into(),
            valid_to: Some("2025-06-01T00:00:00.000000+00:00".into()),
            confidence: 1.0,
            properties: serde_json::Value::Null,
            reason: None,
        },
    )
    .unwrap();

    let active = timeline(&conn, "e_a", false).unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].relation_type, "active");

    let all = timeline(&conn, "e_a", true).unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn add_alias_rewires_relations() {
    let conn = fresh();
    let now = "2026-01-01T00:00:00Z";
    insert_entity(
        &conn,
        &Entity {
            id: "alias".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::Value::Null,
        },
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity {
            id: "canonical".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::Value::Null,
        },
    )
    .unwrap();
    conn.execute("INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json, created_at) VALUES ('alias', 'knows', 'other', 'global', ?1, NULL, 1.0, '{}', ?1)", rusqlite::params![now]).unwrap();

    add_alias(&conn, "alias", "canonical").unwrap();

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM entities WHERE id = 'alias'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "alias entity should be deleted");

    let from: String = conn
        .query_row(
            "SELECT from_id FROM relations WHERE relation_type = 'knows'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(from, "canonical", "relation should be rewired to canonical");
}

#[test]
fn add_alias_carries_over_properties_into_canonical() {
    let conn = fresh();
    insert_entity(
        &conn,
        &Entity {
            id: "canonical".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"name": "Alice", "shared": "canonical_wins"}),
        },
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity {
            id: "alias".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"alt_name": "Liz", "shared": "alias_loses"}),
        },
    )
    .unwrap();

    add_alias(&conn, "alias", "canonical").unwrap();

    let props: String = conn
        .query_row(
            "SELECT properties_json FROM entities WHERE id = 'canonical'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&props).unwrap();
    assert_eq!(parsed["name"], "Alice");
    assert_eq!(
        parsed["alt_name"], "Liz",
        "non-colliding alias props must carry over"
    );
    assert_eq!(
        parsed["shared"], "canonical_wins",
        "canonical wins on collision"
    );
}

#[test]
fn add_alias_cleans_alias_vectors() {
    let conn = fresh();
    conn.execute_batch("CREATE TABLE mem_vectors (mem_id TEXT, embedding BLOB)")
        .unwrap();
    insert_entity(
        &conn,
        &Entity {
            id: "alias".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::Value::Null,
        },
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity {
            id: "canonical".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::Value::Null,
        },
    )
    .unwrap();
    conn.execute(
        "INSERT INTO mem_vectors VALUES ('alias', X''), ('alias_chunk_1', X''), ('canonical', X'')",
        [],
    )
    .unwrap();

    add_alias(&conn, "alias", "canonical").unwrap();

    let alias_vecs: i64 = conn.query_row(
            "SELECT COUNT(*) FROM mem_vectors WHERE mem_id = 'alias' OR mem_id LIKE 'alias_chunk_%'",
            [], |r| r.get(0)
        ).unwrap();
    assert_eq!(
        alias_vecs, 0,
        "alias vectors (incl. chunks) must be deleted"
    );
    let canonical_vecs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mem_vectors WHERE mem_id = 'canonical'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(canonical_vecs, 1, "canonical vectors must stay");
}

#[test]
fn add_alias_writes_audit_log() {
    let conn = fresh();
    insert_entity(
        &conn,
        &Entity {
            id: "alias".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::Value::Null,
        },
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity {
            id: "canonical".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::Value::Null,
        },
    )
    .unwrap();
    insert_entity(
        &conn,
        &Entity {
            id: "other".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::Value::Null,
        },
    )
    .unwrap();
    let now = "2026-01-01T00:00:00Z";
    conn.execute(
            "INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json, created_at)
             VALUES ('alias','knows','other','global',?1,NULL,1.0,'{}',?1),
                    ('other','knows','alias','global',?1,NULL,1.0,'{}',?1)",
            rusqlite::params![now],
        ).unwrap();

    add_alias(&conn, "alias", "canonical").unwrap();

    let (count, actor, details): (i64, String, String) = conn
        .query_row(
            "SELECT count, actor, details_json FROM graph_purge_log ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(actor, "add_alias");
    let parsed: serde_json::Value = serde_json::from_str(&details).unwrap();
    assert_eq!(parsed["op"], "add_alias");
    assert_eq!(parsed["alias_id"], "alias");
    assert_eq!(parsed["canonical_id"], "canonical");
    assert!(
        parsed["rewired_from"].as_u64().unwrap() + parsed["rewired_to"].as_u64().unwrap() >= 2,
        "should account for 2 rewired edges, got: {:?}",
        parsed
    );
    assert!(
        count >= 2,
        "audit count covers rewires + dupes, got {}",
        count
    );
}

#[test]
fn insert_relation_persists_reason() {
    let conn = fresh();
    pair_entities(&conn);
    let r = Relation {
        from_id: "e_a".into(),
        relation_type: "uses".into(),
        to_id: "e_b".into(),
        scope: "global".into(),
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
        confidence: 1.0,
        properties: serde_json::json!({}),
        reason: Some("dreamer-bridge".into()),
    };
    insert_relation(&conn, &r).unwrap();

    let got: Option<String> = conn.query_row(
            "SELECT reason FROM relations WHERE from_id='e_a' AND to_id='e_b' AND relation_type='uses'",
            [],
            |r| r.get(0),
        ).unwrap();
    assert_eq!(got.as_deref(), Some("dreamer-bridge"));
}

#[test]
fn insert_relation_null_reason() {
    let conn = fresh();
    pair_entities(&conn);
    let r = Relation {
        from_id: "e_a".into(),
        relation_type: "uses".into(),
        to_id: "e_b".into(),
        scope: "global".into(),
        valid_from: "2026-01-01T00:00:00Z".into(),
        valid_to: None,
        confidence: 1.0,
        properties: serde_json::json!({}),
        reason: None,
    };
    insert_relation(&conn, &r).unwrap();
    let got: Option<String> = conn
        .query_row(
            "SELECT reason FROM relations WHERE from_id='e_a'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(got.is_none());
}

#[test]
fn iso_t_separator_does_not_leak_through_string_compare() {
    let conn = fresh();
    pair_entities(&conn);
    let dead = Relation {
        from_id: "e_a".into(),
        relation_type: "k".into(),
        to_id: "e_b".into(),
        scope: "global".into(),
        valid_from: "2025-01-01T00:00:00.000000+00:00".into(),
        valid_to: Some("2025-06-01T00:00:00.000000+00:00".into()),
        confidence: 1.0,
        properties: serde_json::Value::Null,
        reason: None,
    };
    insert_relation(&conn, &dead).unwrap();
    let rs = related(&conn, "e_a", None, None, false).unwrap();
    assert!(
        rs.is_empty(),
        "ISO-T-separated invalidated row must NOT leak"
    );
}
