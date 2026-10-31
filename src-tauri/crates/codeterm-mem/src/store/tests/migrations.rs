use super::*;

fn columns(conn: &Connection, table: &str) -> Vec<String> {
    conn.prepare(&format!("PRAGMA table_info({table})"))
        .unwrap()
        .query_map([], |r| r.get(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn versioned_legacy_and_fresh_converge_and_backfill_owner() {
    let legacy = Connection::open_in_memory().unwrap();
    legacy.execute_batch(include_str!("legacy.sql")).unwrap();
    let before = columns(&legacy, "mem_items");
    assert!(!before.contains(&"author_kind".into()));
    init_schema(&legacy).unwrap();
    let fresh = Connection::open_in_memory().unwrap();
    init_schema(&fresh).unwrap();
    let expected = "id uid kind scope title summary area tags_json body_inline body_blob_path source importance created_at updated_at accessed_at access_count recurrence_count source_path source_size source_mtime evidence_count false_fire_count promotion_threshold is_pinned_wakeup seed_fingerprint author_kind author_uid author_friendship author_label".split_whitespace().map(str::to_string).collect::<Vec<_>>();
    assert_eq!(columns(&legacy, "mem_items"), expected);
    assert_eq!(columns(&fresh, "mem_items"), expected);
    assert_eq!(
        columns(&legacy, "mem_item_history"),
        columns(&fresh, "mem_item_history")
    );
    assert_eq!(columns(&legacy,"mem_item_history"), "id item_id changed_at changed_by change_type old_title new_title old_body new_body old_importance new_importance old_kind new_kind reason author_kind author_uid author_friendship author_label".split_whitespace().map(str::to_string).collect::<Vec<_>>());
    for table in ["mem_items", "mem_item_history"] {
        let author: (String, Option<String>) = legacy
            .query_row(
                &format!("SELECT author_kind,author_uid FROM {table}"),
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(author, ("owner".into(), None));
    }
    let version = |c: &Connection| {
        c.query_row("SELECT version FROM mem_schema_version", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap()
    };
    assert_eq!(
        version(&legacy),
        codeterm_mem_store::migrations::SCHEMA_VERSION
    );
    assert_eq!(version(&fresh), version(&legacy));
    let changes = legacy.total_changes();
    init_schema(&legacy).unwrap();
    assert_eq!(legacy.total_changes(), changes);
    println!("legacy before: {before:?}\nlegacy after: {expected:?}");
}

#[test]
fn failed_old_schema_open_preserves_file_and_journal_mode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("knowledge.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(include_str!("legacy.sql")).unwrap();
    conn.execute_batch(
        "CREATE TABLE mem_schema_version(version INTEGER NOT NULL);
         INSERT INTO mem_schema_version VALUES(1);
         ALTER TABLE mem_items ADD COLUMN author_uid TEXT;",
    )
    .unwrap();
    let schema: String = conn
        .query_row("SELECT group_concat(sql) FROM sqlite_master", [], |row| {
            row.get(0)
        })
        .unwrap();
    drop(conn);
    let before = std::fs::read(&path).unwrap();

    let error = match crate::backend::SqliteMemStore::open(&path, &dir.path().join("blobs")) {
        Ok(_) => panic!("conflicting old schema must be refused"),
        Err(error) => error,
    };
    assert!(
        error.contains("duplicate column name: author_uid"),
        "{error}"
    );
    let conn = Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "delete"
    );
    assert_eq!(
        conn.query_row("SELECT group_concat(sql) FROM sqlite_master", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        schema
    );
    assert_eq!(
        conn.query_row("SELECT version FROM mem_schema_version", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(conn);
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
