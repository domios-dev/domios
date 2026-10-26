use super::*;

#[test]
fn schema_backups_preserve_real_fts_and_connection_constraints_without_sharing_rows() {
    let first = memory_connection().unwrap();
    first
        .execute(
            "INSERT INTO mem_fts(mem_id, title) VALUES ('own-id', 'opaque-searchable')",
            [],
        )
        .unwrap();
    assert_eq!(
        first
            .query_row(
                "SELECT count(*) FROM mem_fts WHERE mem_fts MATCH 'searchable'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    let second = memory_connection().unwrap();
    assert_eq!(
        second
            .query_row("SELECT count(*) FROM mem_fts", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    for conn in [&first, &second] {
        assert_eq!(
            conn.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row("PRAGMA synchronous", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
    }
}
