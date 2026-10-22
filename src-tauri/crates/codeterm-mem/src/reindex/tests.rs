use super::*;

#[test]
fn embeddable_requires_real_body() {
    assert!(!body_is_embeddable(""));
    assert!(!body_is_embeddable("   \n  "));
    assert!(!body_is_embeddable("too short"));
    assert!(body_is_embeddable("a genuinely meaningful body of content"));
}

#[test]
fn blueprint_embedding_starts_with_intent_and_describes_structure() {
    let document = codeterm_blueprint_types::blueprint_document::Document::from_definition(
        &serde_json::json!({"spec": {"name":"fixture", "owner":"team",
        "trigger":{"type":"schedule","every":"1m"},
        "steps":[{"name":"Echo", "input":"text", "kind":{"kind":"command", "command":"echo private"}}],
        "result":{"to":"notify"}}})
    ).unwrap();
    let document = codeterm_blueprint_types::blueprint_memory::abstract_document(document);
    let text = blueprint_description(&document, "Detect stalled deployments");
    assert!(text.starts_with("Detect stalled deployments\n"));
    for term in ["schedule", "command", "notify"] {
        assert!(text.contains(term));
    }
    assert!(!text.contains("echo private"));
    #[cfg(feature = "embeddings")]
    assert_eq!(
        text,
        compose_item_embed_text(
            "skill",
            "ignored",
            None,
            &[],
            &document.yaml().unwrap(),
            Some("blueprint-library"),
            Some("Detect stalled deployments")
        )
    );
}

#[cfg(feature = "embeddings")]
#[test]
fn reindex_existing_blueprint_uses_saved_intent_in_canonical_description() {
    let mem = crate::MemManager::open_in_memory().unwrap();
    let document = codeterm_blueprint_types::blueprint_document::Document::from_definition(
        &serde_json::json!({"create":{"name":"watch-ci", "probe":"echo ready", "question":"Ready?",
        "buckets":["done"], "routes":{"done":"notify"}, "every":"1m", "until":"done", "timeout":"1h"}})
    ).unwrap();
    let id = mem
        .save_blueprint(
            "project",
            document,
            &["Detect frozen releases".into()],
            codeterm_blueprint_types::blueprint_memory::Evidence {
                kept: true,
                successes: 1,
                ..Default::default()
            },
        )
        .unwrap()
        .id;
    let conn = mem.conn.lock();
    crate::embeddings::init_extension(&conn).unwrap();
    let accesses = |conn: &rusqlite::Connection| {
        conn.query_row(
            "SELECT access_count FROM mem_items WHERE id=?1",
            [&id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
    };
    let before = accesses(&conn);
    let missing = query_missing_vectors(&conn, &mem.blobs_dir);
    assert_eq!(accesses(&conn), before);
    let (_, text) = missing.iter().find(|(saved, _)| saved == &id).unwrap();
    assert_eq!(
        text,
        "Detect frozen releases\nNode kinds: command\nSources: probe\nSinks: notify"
    );
    assert!(!text.contains("schema_version"));
    assert!(!text.contains("echo ready"));
}
