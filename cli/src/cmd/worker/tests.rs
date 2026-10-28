use super::*;
use rusqlite::Connection;

/// Pre-v1.1.23 schema. Mirrors `codeterm_mem_store::tests::init_v1_22_schema`
/// but inlined here because that helper is `pub(crate)` to mem-store.
fn init_v1_22_schema(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE relations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_id TEXT NOT NULL,
            relation_type TEXT NOT NULL,
            to_id TEXT NOT NULL,
            scope TEXT NOT NULL,
            valid_from TEXT NOT NULL,
            valid_to TEXT,
            confidence REAL NOT NULL DEFAULT 1.0,
            properties_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            UNIQUE(from_id, relation_type, to_id, valid_from)
        );
        CREATE TABLE entities (
            id TEXT PRIMARY KEY,
            entity_type TEXT NOT NULL,
            scope TEXT NOT NULL,
            properties_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE entity_aliases (
            alias_id TEXT PRIMARY KEY,
            canonical_id TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE TABLE mem_items (
            id TEXT PRIMARY KEY, uid TEXT NOT NULL UNIQUE,
            kind TEXT NOT NULL, scope TEXT NOT NULL,
            title TEXT NOT NULL, summary TEXT, area TEXT,
            tags_json TEXT NOT NULL DEFAULT '[]',
            body_inline TEXT, body_blob_path TEXT,
            source TEXT, importance REAL NOT NULL DEFAULT 3.0,
            created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
            accessed_at TEXT, access_count INTEGER NOT NULL DEFAULT 0,
            recurrence_count INTEGER NOT NULL DEFAULT 0
        );",
    )
    .unwrap();
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({})", table))
        .unwrap();
    let names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    names.iter().any(|name| name == column)
}

fn unique_temp_db_path() -> std::path::PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("codeterm-worker-test-{}-{}.db", pid, nanos))
}

/// `mem run-worker` against a pre-v1.1.23 backup must add the new
/// `false_fire_count` column on open. Without this, lesson promotion
/// crashes with "no such column: false_fire_count".
#[test]
fn open_worker_db_runs_v1_23_migrations_on_legacy_schema() {
    let path = unique_temp_db_path();
    let _ = std::fs::remove_file(&path);

    // Bootstrap a v1.22-era schema and seed a single lesson row.
    {
        let conn = Connection::open(&path).unwrap();
        init_v1_22_schema(&conn);
        assert!(!column_exists(&conn, "mem_items", "false_fire_count"));
        conn.execute(
            "INSERT INTO mem_items
               (id, uid, kind, scope, title, tags_json,
                body_inline, importance, created_at, updated_at)
               VALUES ('lesson_x', 'uid-x', 'lesson', 'global', 'L',
                       '[]', 'body', 3.0,
                       '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    }

    // Open via the worker bootstrap — should run migrations.
    let conn = open_worker_db(&path).expect("open_worker_db");
    assert!(
        column_exists(&conn, "mem_items", "false_fire_count"),
        "false_fire_count column must exist after open_worker_db",
    );

    // Sanity: the seeded row survived the migration.
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mem_items WHERE kind = 'lesson'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);

    drop(conn);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
}

#[test]
fn build_skill_id_ascii() {
    let id = codeterm_mem_store::skill_folders::skill_id_for_title("My Skill");
    assert_eq!(id, "skill_my_skill");
}

#[test]
fn build_skill_id_non_ascii_does_not_panic() {
    // Non-ASCII can be 3-byte; keep guard to avoid panic on byte slicing.
    let long_chinese = "技能技能技能技能技能技能技能技能技能技能技能技能技能技能"; // 14 × 3 = 42 bytes as slug
    let result = std::panic::catch_unwind(|| {
        codeterm_mem_store::skill_folders::skill_id_for_title(long_chinese)
    });
    assert!(result.is_ok(), "should not panic on non-ASCII skill names");
}

#[test]
fn build_skill_id_emoji_does_not_panic() {
    let emoji_title = "🚀🎯🔥🌟💡🎨🛡️🔑🌈✨🎭🎪🎡🎢🎠🎟🎃🎄🎆🎇"; // many emoji
    let result = std::panic::catch_unwind(|| {
        codeterm_mem_store::skill_folders::skill_id_for_title(emoji_title)
    });
    assert!(result.is_ok(), "should not panic on emoji skill names");
}
