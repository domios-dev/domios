use super::{LIFECYCLE_SCHEMA, SCHEMA};
use rusqlite::Connection;

#[test]
fn lifecycle_migration_keeps_existing_rows_locked_with_their_progress() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    conn.execute_batch(
        "INSERT INTO tab_todo_heads VALUES ('tab', 1);
         INSERT INTO tab_todo_generations VALUES ('tab', 1, 'ws', NULL, 'active', 10, 3);
         INSERT INTO tab_todo_items VALUES ('tab', 1, 'done-item', 0, 'Done', NULL, 'done', 2, 0, NULL);
         INSERT INTO tab_todo_items VALUES ('tab', 1, 'open-item', 1, 'Open', NULL, 'open', 1, 0, NULL);
         INSERT INTO tab_todo_items VALUES ('tab', 1, 'gone', 2, 'Gone', NULL, 'open', 2, 1, NULL);
         INSERT INTO tab_todo_audit (tab_id, generation, item_id, actor_tab_id, action, item_revision, happened_at)
             VALUES ('tab', 1, 'done-item', NULL, 'state_change', 2, 11);",
    )
    .unwrap();
    conn.execute_batch(LIFECYCLE_SCHEMA).unwrap();
    let rows: Vec<(String, bool, i64, i64, i64)> = conn
        .prepare("SELECT item_id, locked, attempt, started_count, completed_count FROM tab_todo_items ORDER BY position")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        rows,
        [
            ("done-item".to_string(), true, 1, 1, 1),
            ("open-item".to_string(), true, 0, 0, 0),
            ("gone".to_string(), false, 0, 0, 0),
        ]
    );
    let audit: i64 = conn
        .query_row("SELECT COUNT(*) FROM tab_todo_audit", [], |row| row.get(0))
        .unwrap();
    assert_eq!(audit, 1);
    assert!(conn
        .execute(
            "INSERT INTO tab_todo_items (tab_id, generation, item_id, position, title, state, revision, deleted, locked, attempt, attempt_closed, started_count, completed_count, reopened_count, blocked_count, skipped_count, folded_events) VALUES ('tab', 1, 'thirteenth', 12, 'T', 'open', 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0)",
            [],
        )
        .is_err());
    assert!(conn
        .execute(
            "UPDATE tab_todo_items SET deleted = 1 WHERE item_id = 'done-item'",
            [],
        )
        .is_err());
}
