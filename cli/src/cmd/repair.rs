// `mem repair` — opens knowledge.db directly so it works while the
// daemon is wedged. SQL stays narrow: never bulk-deletes.

use crate::data_dir_name;
use crate::MemRepairAction;

pub fn run_mem_repair(action: &MemRepairAction) {
    let result = match action {
        MemRepairAction::ReviveBrokenRelations { since, dry_run, db_path } => {
            try_revive(since.as_deref(), *dry_run, db_path.as_deref())
        }
    };
    match result {
        Ok(json) => println!("{}", json),
        Err(e) => {
            println!("{}", serde_json::json!({"ok": false, "error": e}));
            std::process::exit(1);
        }
    }
}

fn try_revive(
    since: Option<&str>,
    dry_run: bool,
    db_path_arg: Option<&str>,
) -> Result<serde_json::Value, String> {
    let db_path = match db_path_arg {
        Some(p) => std::path::PathBuf::from(p),
        None => default_db_path()?,
    };
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("open {}: {}", db_path.display(), e))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| format!("busy_timeout: {}", e))?;

    let count = revive_broken_relations(&conn, since, dry_run)?;
    Ok(serde_json::json!({
        "ok": true,
        "revived": count,
        "dry_run": dry_run,
        "since": since,
    }))
}

fn default_db_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("home dir not found")?;
    Ok(home.join(data_dir_name()).join("mem").join("knowledge.db"))
}

/// Restore `valid_to = NULL` for relations whose endpoints both still exist
/// in `entities` or `mem_items`. `since` filters by `valid_to >= since`
/// (RFC 3339). Returns count; mutates iff `!dry_run`.
fn revive_broken_relations(
    conn: &rusqlite::Connection,
    since: Option<&str>,
    dry_run: bool,
) -> Result<u64, String> {
    const WHERE_BASE: &str = "valid_to IS NOT NULL \
         AND from_id IN (SELECT id FROM entities UNION SELECT id FROM mem_items) \
         AND to_id IN (SELECT id FROM entities UNION SELECT id FROM mem_items)";
    let since_clause = if since.is_some() { " AND valid_to >= ?1" } else { "" };
    let params: Vec<&dyn rusqlite::ToSql> =
        since.iter().map(|s| s as &dyn rusqlite::ToSql).collect();

    let count: i64 = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM relations WHERE {}{}", WHERE_BASE, since_clause),
            params.as_slice(),
            |r| r.get(0),
        )
        .map_err(|e| format!("count: {}", e))?;

    if !dry_run && count > 0 {
        conn.execute(
            &format!("UPDATE relations SET valid_to = NULL WHERE {}{}", WHERE_BASE, since_clause),
            params.as_slice(),
        )
        .map_err(|e| format!("update: {}", e))?;
    }
    Ok(count as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};

    fn setup_schema(conn: &Connection) {
        conn.execute_batch(
            r#"
            CREATE TABLE entities (
                id TEXT PRIMARY KEY,
                entity_type TEXT NOT NULL,
                scope TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE mem_items (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                title TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE relations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                from_id TEXT NOT NULL,
                relation_type TEXT NOT NULL,
                to_id TEXT NOT NULL,
                scope TEXT NOT NULL,
                valid_from TEXT NOT NULL,
                valid_to TEXT,
                created_at TEXT NOT NULL
            );
            "#,
        )
        .unwrap();
    }

    fn insert_entity(conn: &Connection, id: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO entities (id, entity_type, scope, created_at, updated_at) \
             VALUES (?1, 'thing', 'global', ?2, ?2)",
            params![id, &now],
        )
        .unwrap();
    }

    fn insert_mem_item(conn: &Connection, id: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO mem_items (id, kind, title, created_at, updated_at) \
             VALUES (?1, 'fact', 'x', ?2, ?2)",
            params![id, &now],
        )
        .unwrap();
    }

    fn insert_invalidated(conn: &Connection, from: &str, to: &str, valid_to: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, created_at) \
             VALUES (?1, 'mentions', ?2, 'global', ?3, ?4, ?3)",
            params![from, to, &now, valid_to],
        )
        .unwrap();
    }

    fn valid_to_of(conn: &Connection, from: &str, to: &str) -> Option<String> {
        conn.query_row(
            "SELECT valid_to FROM relations WHERE from_id = ?1 AND to_id = ?2",
            params![from, to],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn revives_when_both_endpoints_exist_in_entities() {
        let conn = Connection::open_in_memory().unwrap();
        setup_schema(&conn);
        insert_entity(&conn, "e_a");
        insert_entity(&conn, "e_b");
        insert_invalidated(&conn, "e_a", "e_b", &chrono::Utc::now().to_rfc3339());

        assert_eq!(revive_broken_relations(&conn, None, false).unwrap(), 1);
        assert!(valid_to_of(&conn, "e_a", "e_b").is_none());
    }

    #[test]
    fn revives_mixed_entity_and_mem_item_endpoints() {
        let conn = Connection::open_in_memory().unwrap();
        setup_schema(&conn);
        insert_mem_item(&conn, "fact_rust");
        insert_entity(&conn, "tech_rust");
        insert_invalidated(&conn, "fact_rust", "tech_rust", &chrono::Utc::now().to_rfc3339());

        assert_eq!(revive_broken_relations(&conn, None, false).unwrap(), 1);
        assert!(valid_to_of(&conn, "fact_rust", "tech_rust").is_none());
    }

    #[test]
    fn skips_relation_with_genuinely_missing_endpoint() {
        let conn = Connection::open_in_memory().unwrap();
        setup_schema(&conn);
        insert_entity(&conn, "e_a");
        // 'ghost' missing from both tables
        insert_invalidated(&conn, "e_a", "ghost", &chrono::Utc::now().to_rfc3339());

        assert_eq!(revive_broken_relations(&conn, None, false).unwrap(), 0);
        assert!(valid_to_of(&conn, "e_a", "ghost").is_some());
    }

    #[test]
    fn dry_run_counts_without_mutating() {
        let conn = Connection::open_in_memory().unwrap();
        setup_schema(&conn);
        insert_entity(&conn, "e_a");
        insert_entity(&conn, "e_b");
        insert_invalidated(&conn, "e_a", "e_b", &chrono::Utc::now().to_rfc3339());

        assert_eq!(revive_broken_relations(&conn, None, true).unwrap(), 1);
        assert!(valid_to_of(&conn, "e_a", "e_b").is_some());
    }

    #[test]
    fn since_filter_excludes_older_invalidations() {
        let conn = Connection::open_in_memory().unwrap();
        setup_schema(&conn);
        insert_entity(&conn, "e_a");
        insert_entity(&conn, "e_b");
        insert_entity(&conn, "e_c");
        insert_invalidated(&conn, "e_a", "e_b", "2026-04-01T00:00:00Z");
        insert_invalidated(&conn, "e_a", "e_c", "2026-05-02T00:00:00Z");

        assert_eq!(
            revive_broken_relations(&conn, Some("2026-05-01T00:00:00Z"), false).unwrap(),
            1
        );
        assert!(valid_to_of(&conn, "e_a", "e_b").is_some());
        assert!(valid_to_of(&conn, "e_a", "e_c").is_none());
    }

    #[test]
    fn ignores_active_relations() {
        let conn = Connection::open_in_memory().unwrap();
        setup_schema(&conn);
        insert_entity(&conn, "e_a");
        insert_entity(&conn, "e_b");
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, created_at) \
             VALUES ('e_a', 'mentions', 'e_b', 'global', ?1, NULL, ?1)",
            params![&now],
        )
        .unwrap();

        assert_eq!(revive_broken_relations(&conn, None, false).unwrap(), 0);
    }
}
