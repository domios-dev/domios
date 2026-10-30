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
fn fresh_and_current_reopen_keep_complete_schema() {
    let fresh = Connection::open_in_memory().unwrap();
    init_schema(&fresh).unwrap();
    let expected = "id uid kind scope title summary area tags_json body_inline body_blob_path source importance created_at updated_at accessed_at access_count recurrence_count source_path source_size source_mtime evidence_count false_fire_count promotion_threshold is_pinned_wakeup seed_fingerprint author_kind author_uid author_friendship author_label".split_whitespace().map(str::to_string).collect::<Vec<_>>();
    assert_eq!(columns(&fresh, "mem_items"), expected);
    assert_eq!(columns(&fresh,"mem_item_history"), "id item_id changed_at changed_by change_type old_title new_title old_body new_body old_importance new_importance old_kind new_kind reason author_kind author_uid author_friendship author_label".split_whitespace().map(str::to_string).collect::<Vec<_>>());
    let schema: String = fresh.query_row("SELECT group_concat(sql) FROM sqlite_master", [], |row| row.get(0)).unwrap();
    let changes = fresh.total_changes();
    init_schema(&fresh).unwrap();
    assert_eq!(fresh.total_changes(), changes);
    assert_eq!(fresh.query_row("SELECT group_concat(sql) FROM sqlite_master", [], |row| row.get::<_, String>(0)).unwrap(), schema);
    assert_eq!(fresh.query_row("SELECT version FROM mem_schema_version", [], |row| row.get::<_, i64>(0)).unwrap(), 2);
}

#[test]
fn legacy_schema_is_refused_without_backfilling_authors() {
    let legacy = Connection::open_in_memory().unwrap();
    legacy.execute_batch(include_str!("legacy.sql")).unwrap();
    let before = columns(&legacy, "mem_items");
    let changes = legacy.total_changes();
    assert!(init_schema(&legacy).unwrap_err().to_string().contains("unstamped memory schema refused"));
    assert_eq!(columns(&legacy, "mem_items"), before);
    assert_eq!(legacy.total_changes(), changes);
}
