// `mem run-worker` processes one dreaming/librarian batch directly in knowledge.db.
// It exits after one pass; supervisor runs it to isolate maintenance failures.

use crate::data_dir_name;

pub fn run_mem_worker(kind: &str, db_path_arg: Option<&str>) {
    let db_path = match db_path_arg {
        Some(p) => std::path::PathBuf::from(p),
        None => match resolve_default_db_path() {
            Ok(p) => p,
            Err(e) => {
                worker_emit_error(&e);
                std::process::exit(1);
            }
        },
    };

    register_sqlite_vec_extension();

    let conn = match open_worker_db(&db_path) {
        Ok(c) => c,
        Err(e) => {
            worker_emit_error(&e);
            std::process::exit(1);
        }
    };

    let result = match kind {
        "dreaming-light" => worker_dreaming_light(&conn).map(|r| {
            serde_json::json!({
                "considered": r.considered, "promoted": r.promoted,
                "dropped": r.dropped, "phase": r.phase,
                "lessons_promoted": r.lessons_promoted,
                "lessons_already_high_confidence": r.lessons_already_high_confidence,
            })
        }),
        "dreaming-deep" => worker_dreaming_deep(&conn).map(|r| {
            serde_json::json!({
                "considered": r.considered, "promoted": r.promoted,
                "dropped": r.dropped, "phase": r.phase,
            })
        }),
        "dreaming-normalize" => worker_dreaming_normalize(&conn),
        "librarian-once" => {
            let skills_root = dirs::home_dir()
                .unwrap_or_default()
                .join(data_dir_name())
                .join("skills");
            worker_librarian_once(&conn, &skills_root).map(|r| serde_json::json!(r))
        }
        other => Err(format!("unknown worker kind: {}", other)),
    };

    match result {
        Ok(report) => {
            println!("{}", serde_json::json!({"ok": true, "report": report}));
        }
        Err(e) => {
            worker_emit_error(&e);
            std::process::exit(1);
        }
    }
}

fn worker_emit_error(msg: &str) {
    println!("{}", serde_json::json!({"ok": false, "error": msg}));
}

fn resolve_default_db_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("home dir not found")?;
    Ok(home.join(data_dir_name()).join("mem").join("knowledge.db"))
}

/// Open a knowledge.db connection, set busy_timeout, run codeterm-mem-store
/// migrations, and apply the worker's PRAGMAs. Migrations are idempotent;
/// running them on every worker invocation is the cheap way to guarantee
/// the schema matches the binary even when the DB is an older backup.
pub(crate) fn open_worker_db(db_path: &std::path::Path) -> Result<rusqlite::Connection, String> {
    let conn = rusqlite::Connection::open(db_path)
        .map_err(|e| format!("open {}: {}", db_path.display(), e))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| format!("busy_timeout: {}", e))?;
    codeterm_mem_store::migrations::run(&conn).map_err(|e| format!("migrations: {}", e))?;
    conn.execute_batch("PRAGMA synchronous = NORMAL;")
        .map_err(|e| format!("synchronous pragma: {}", e))?;
    Ok(conn)
}

fn register_sqlite_vec_extension() {
    use std::sync::Once;
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| unsafe {
        let init = std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut std::os::raw::c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> i32,
        >(sqlite_vec::sqlite3_vec_init as *const ());
        rusqlite::ffi::sqlite3_auto_extension(Some(init));
    });
}

#[derive(Debug, Clone)]
struct WorkerDreamReport {
    considered: u64,
    promoted: u64,
    dropped: u64,
    phase: String,
    lessons_promoted: u64,
    lessons_already_high_confidence: u64,
}

fn worker_dreaming_light(conn: &rusqlite::Connection) -> Result<WorkerDreamReport, String> {
    let now_ts = chrono::Utc::now();
    let one_day_ago = (now_ts - chrono::Duration::days(1)).to_rfc3339();

    let candidates: Vec<(String, i64, f64, String, Option<String>)> = {
        let mut stmt = conn
            .prepare(
                r#"SELECT id, access_count, importance, created_at, accessed_at
               FROM mem_items
               WHERE kind = 'learning'
                 AND access_count >= 3
                 AND created_at < ?1"#,
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![one_day_ago], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, f64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        rows.filter_map(|r| r.ok()).collect()
    };

    let mut considered = 0u64;
    let mut promoted = 0u64;

    for (id, access_count, importance, created_at, accessed_at) in &candidates {
        considered += 1;

        let frequency = (*access_count as f64 / 10.0).min(1.0);
        let recency = accessed_at
            .as_ref()
            .and_then(|a| chrono::DateTime::parse_from_rfc3339(a).ok())
            .map(|a| {
                let days =
                    (now_ts - a.with_timezone(&chrono::Utc)).num_hours().max(0) as f64 / 24.0;
                (-days * 0.693 / 14.0).exp()
            })
            .unwrap_or(0.0);
        let imp_score = (*importance / 5.0).min(1.0);
        let age = chrono::DateTime::parse_from_rfc3339(created_at)
            .ok()
            .map(|c| {
                let days =
                    (now_ts - c.with_timezone(&chrono::Utc)).num_hours().max(0) as f64 / 24.0;
                (days / 7.0).min(1.0)
            })
            .unwrap_or(0.5);

        let score = frequency * 0.30 + recency * 0.25 + imp_score * 0.25 + age * 0.20;
        if score < 0.5 {
            continue;
        }

        let now = chrono::Utc::now().to_rfc3339();
        let n = conn.execute(
            "UPDATE mem_items SET kind = 'fact', importance = MAX(importance, 4.0), updated_at = ?1 WHERE id = ?2",
            rusqlite::params![&now, id],
        ).map_err(|e| e.to_string())?;
        if n > 0 {
            // history_log mirror — best-effort, don't fail the run on history errors.
            let new_imp: f64 = importance.max(4.0);
            let _ = conn.execute(
                r#"INSERT INTO mem_item_history
                   (item_id, changed_at, changed_by, change_type,
                    old_importance, new_importance, old_kind, new_kind)
                   VALUES (?1, ?2, 'dreaming:rem', 'promote', ?3, ?4, 'learning', 'fact')"#,
                rusqlite::params![id, &now, importance, new_imp],
            );
            promoted += 1;
        }
    }

    let machine = local_machine_id();
    let promo = codeterm_mem_store::lesson::promote_high_confidence_lessons(
        conn,
        codeterm_mem_store::lesson::DEFAULT_PROMOTION_THRESHOLD,
        &machine,
    )
    .map_err(|e| format!("lesson promotion: {}", e))?;

    Ok(WorkerDreamReport {
        considered,
        promoted,
        dropped: 0,
        phase: "light".into(),
        lessons_promoted: promo.promoted,
        lessons_already_high_confidence: promo.already_high_confidence,
    })
}

/// Local machine identifier for lesson_audit. Reads `instance_id` from the
/// mesh identity file when present; falls back to "unknown" so the worker
/// stays useful before the user has set up mesh.
fn local_machine_id() -> String {
    let path = dirs::home_dir().map(|h| {
        h.join(crate::data_dir_name())
            .join("mesh")
            .join("identity.json")
    });
    let Some(p) = path else {
        return "unknown".into();
    };
    let Ok(content) = std::fs::read_to_string(&p) else {
        return "unknown".into();
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) else {
        return "unknown".into();
    };
    val.get("instance_id")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| "unknown".into())
}

fn worker_dreaming_deep(conn: &rusqlite::Connection) -> Result<WorkerDreamReport, String> {
    let tiers: &[(&str, i64)] = &[
        ("conversation", 30),
        ("daily", 30),
        ("note", 60),
        ("script", 60),
        ("learning", 90),
    ];
    let mut total_expired = 0u64;
    for (kind, days) in tiers {
        let now = chrono::Utc::now().to_rfc3339();
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(*days)).to_rfc3339();
        let n = conn
            .execute(
                "UPDATE mem_items SET importance = 0.0, updated_at = ?1 \
             WHERE kind = ?2 AND access_count = 0 AND created_at < ?3 \
             AND importance > 0.0 AND importance < 4.0",
                rusqlite::params![&now, kind, &cutoff],
            )
            .map_err(|e| e.to_string())? as u64;
        total_expired += n;
    }
    Ok(WorkerDreamReport {
        considered: total_expired,
        promoted: 0,
        dropped: total_expired,
        phase: "deep".into(),
        lessons_promoted: 0,
        lessons_already_high_confidence: 0,
    })
}

// Graph normalize phase:
// Policy and Levenshtein helpers are local; Graph SQL stays in codeterm-mem-store.

const NORMALIZE_PROTECTED_TYPES: &[&str] = &["kind", "area", "agent", "workspace"];
const NORMALIZE_ORPHAN_AGE_DAYS: i64 = 14;
const NORMALIZE_RATIO_HARD: f64 = 0.85;
const NORMALIZE_RATIO_FLAG: f64 = 0.70;
const NORMALIZE_SYNONYMS: &[(&str, &str)] = &[
    ("references", "mentions"),
    ("refers_to", "mentions"),
    ("contributes_to", "works_on"),
    ("at_location", "located_at"),
];

fn lev(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr = vec![0usize; m + 1];
    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

fn lev_ratio(a: &str, b: &str) -> f64 {
    let max = a.chars().count().max(b.chars().count());
    if max == 0 {
        return 1.0;
    }
    1.0 - (lev(a, b) as f64 / max as f64)
}

fn split_prefix(id: &str) -> Option<(&str, &str)> {
    id.find('_').map(|i| (&id[..i], &id[i + 1..]))
}

fn worker_dreaming_normalize(conn: &rusqlite::Connection) -> Result<serde_json::Value, String> {
    let now = chrono::Utc::now().to_rfc3339();
    let cutoff =
        (chrono::Utc::now() - chrono::Duration::days(NORMALIZE_ORPHAN_AGE_DAYS)).to_rfc3339();

    let relations_canonicalized =
        codeterm_mem_store::graph::canonicalize_relation_types(conn, NORMALIZE_SYNONYMS)
            .map_err(|e| format!("canonicalize: {}", e))?;

    let (merged, flagged) = run_merge_pass(conn).map_err(|e| format!("merge pass: {}", e))?;

    let orphans_deleted = codeterm_mem_store::graph::cleanup_orphan_entities(
        conn,
        NORMALIZE_PROTECTED_TYPES,
        &cutoff,
    )
    .map_err(|e| format!("orphan cleanup: {}", e))?;

    let broken_relations_invalidated =
        codeterm_mem_store::graph::invalidate_broken_relations(conn, &now)
            .map_err(|e| format!("broken invalidate: {}", e))?;

    // Symmetric pass revives only edges with both endpoints still present.
    let revive_report = codeterm_mem_store::graph::revive_broken(
        conn,
        codeterm_mem_store::graph::ReviveBrokenOptions {
            since: None,
            dry_run: false,
        },
    )
    .map_err(|e| format!("broken revive: {}", e))?;

    Ok(serde_json::json!({
        "phase": "normalize",
        "merged": merged,
        "flagged_entity_pairs": flagged,
        "relations_canonicalized": relations_canonicalized,
        "orphans_deleted": orphans_deleted,
        "broken_relations_invalidated": broken_relations_invalidated,
        "broken_relations_revived": revive_report.revived,
    }))
}

fn run_merge_pass(conn: &rusqlite::Connection) -> Result<(u64, Vec<serde_json::Value>), String> {
    let entities =
        codeterm_mem_store::graph::list_entities_with_degree(conn).map_err(|e| e.to_string())?;

    use std::collections::HashMap;
    type MergeBuckets = HashMap<(String, String), Vec<(String, String, u32)>>;
    let mut buckets: MergeBuckets = HashMap::new();
    for e in &entities {
        if let Some((prefix, suffix)) = split_prefix(&e.id) {
            if suffix.chars().count() >= 2 {
                buckets
                    .entry((e.entity_type.clone(), prefix.to_string()))
                    .or_default()
                    .push((e.id.clone(), suffix.to_string(), e.degree));
            }
        }
    }

    let mut merged: u64 = 0;
    let mut flagged: Vec<serde_json::Value> = Vec::new();
    for entries in buckets.values() {
        for i in 0..entries.len() {
            for j in (i + 1)..entries.len() {
                let (a_id, a_suf, a_deg) = &entries[i];
                let (b_id, b_suf, b_deg) = &entries[j];
                let ratio = lev_ratio(a_suf, b_suf);
                if ratio < NORMALIZE_RATIO_FLAG {
                    continue;
                }
                if ratio >= NORMALIZE_RATIO_HARD && a_deg != b_deg {
                    let (alias, canonical) = if a_deg < b_deg {
                        (a_id.clone(), b_id.clone())
                    } else {
                        (b_id.clone(), a_id.clone())
                    };
                    if codeterm_mem_store::graph::add_alias_with_rewire(conn, &alias, &canonical)
                        .is_ok()
                    {
                        merged += 1;
                    }
                } else {
                    flagged.push(serde_json::json!({
                        "a": a_id, "b": b_id, "ratio": ratio,
                    }));
                }
            }
        }
    }
    Ok((merged, flagged))
}

#[derive(Debug, Default, serde::Serialize)]
struct WorkerLibrarianReport {
    skill_folders_registered: usize,
    skill_folders_merged: Vec<codeterm_mem_store::skill_folders::MergedSkill>,
    skill_folders_tombstoned: usize,
    files_re_indexed: usize,
    files_unchanged: usize,
    source_missing: usize,
    source_recovered: usize,
    errors: Vec<String>,
}

fn worker_librarian_once(
    conn: &rusqlite::Connection,
    skills_root: &std::path::Path,
) -> Result<WorkerLibrarianReport, String> {
    let mut report = WorkerLibrarianReport::default();
    let folders = codeterm_mem_store::skill_folders::sync_skill_folders(conn, skills_root);
    report.skill_folders_registered = folders.registered.len();
    report.skill_folders_merged = folders.merged;
    report.skill_folders_tombstoned = folders.skipped_tombstoned.len();
    report.errors.extend(folders.errors);
    librarian_sync_files(conn, &mut report)?;
    Ok(report)
}

struct LibrarianTrackedItem {
    id: String,
    source_path: String,
    source_size: Option<i64>,
    source_mtime: Option<String>,
    tags_json: String,
}

fn librarian_sync_files(
    conn: &rusqlite::Connection,
    report: &mut WorkerLibrarianReport,
) -> Result<(), String> {
    let items: Vec<LibrarianTrackedItem> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, source_path, source_size, source_mtime, tags_json \
             FROM mem_items WHERE source_path IS NOT NULL",
            )
            .map_err(|e| format!("prepare: {}", e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(LibrarianTrackedItem {
                    id: row.get(0)?,
                    source_path: row.get(1)?,
                    source_size: row.get(2)?,
                    source_mtime: row.get(3)?,
                    tags_json: row.get(4)?,
                })
            })
            .map_err(|e| format!("query: {}", e))?;
        rows.filter_map(|r| r.ok()).collect()
    };

    for item in &items {
        match librarian_sync_one(conn, item) {
            Ok(action) => match action {
                LibrarianSyncAction::Unchanged => report.files_unchanged += 1,
                LibrarianSyncAction::ReIndexed => report.files_re_indexed += 1,
                LibrarianSyncAction::Missing => report.source_missing += 1,
                LibrarianSyncAction::Recovered => report.source_recovered += 1,
            },
            Err(e) => report.errors.push(format!("{}: {}", item.id, e)),
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum LibrarianSyncAction {
    Unchanged,
    ReIndexed,
    Missing,
    Recovered,
}

fn librarian_sync_one(
    conn: &rusqlite::Connection,
    item: &LibrarianTrackedItem,
) -> Result<LibrarianSyncAction, String> {
    let raw_path = item
        .source_path
        .strip_prefix("file:")
        .unwrap_or(&item.source_path);
    let path = std::path::Path::new(raw_path);
    let has_missing_tag = librarian_has_tag(&item.tags_json, "source-missing");

    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => {
            if !has_missing_tag {
                librarian_add_tag(conn, &item.id, "source-missing")?;
            }
            return Ok(LibrarianSyncAction::Missing);
        }
    };

    let current_size = meta.len() as i64;
    let current_mtime = meta
        .modified()
        .ok()
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
    let size_match = item.source_size == Some(current_size);
    let mtime_match = match (&item.source_mtime, &current_mtime) {
        (Some(stored), Some(current)) => stored == current,
        _ => false,
    };
    if size_match && mtime_match && !has_missing_tag {
        return Ok(LibrarianSyncAction::Unchanged);
    }

    let content =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {}", item.source_path, e))?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE mem_items SET body_inline = ?1, body_blob_path = NULL, \
         source_size = ?2, source_mtime = ?3, updated_at = ?4 \
         WHERE id = ?5",
        rusqlite::params![&content, current_size, &current_mtime, &now, &item.id],
    )
    .map_err(|e| format!("update: {}", e))?;
    conn.execute(
        "UPDATE mem_fts SET body = ?1 WHERE mem_id = ?2",
        rusqlite::params![&content, &item.id],
    )
    .map_err(|e| format!("update fts: {}", e))?;
    // Drop stale embeddings so next startup tick can rebuild vector index.
    let _ = conn.execute(
        "DELETE FROM mem_vectors WHERE mem_id = ?1",
        rusqlite::params![&item.id],
    );

    if has_missing_tag {
        librarian_remove_tag(conn, &item.id, "source-missing")?;
        return Ok(LibrarianSyncAction::Recovered);
    }
    Ok(LibrarianSyncAction::ReIndexed)
}

fn librarian_has_tag(tags_json: &str, tag: &str) -> bool {
    serde_json::from_str::<Vec<String>>(tags_json)
        .unwrap_or_default()
        .iter()
        .any(|t| t == tag)
}

fn librarian_add_tag(conn: &rusqlite::Connection, item_id: &str, tag: &str) -> Result<(), String> {
    let current: String = conn
        .query_row(
            "SELECT tags_json FROM mem_items WHERE id = ?1",
            [item_id],
            |r| r.get(0),
        )
        .map_err(|e| format!("read tags: {}", e))?;
    let mut tags: Vec<String> = serde_json::from_str(&current).unwrap_or_default();
    if !tags.iter().any(|t| t == tag) {
        tags.push(tag.to_string());
        let new_json = serde_json::to_string(&tags).map_err(|e| format!("serialize: {}", e))?;
        conn.execute(
            "UPDATE mem_items SET tags_json = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![&new_json, chrono::Utc::now().to_rfc3339(), item_id],
        )
        .map_err(|e| format!("update tags: {}", e))?;
    }
    Ok(())
}

fn librarian_remove_tag(
    conn: &rusqlite::Connection,
    item_id: &str,
    tag: &str,
) -> Result<(), String> {
    let current: String = conn
        .query_row(
            "SELECT tags_json FROM mem_items WHERE id = ?1",
            [item_id],
            |r| r.get(0),
        )
        .map_err(|e| format!("read tags: {}", e))?;
    let mut tags: Vec<String> = serde_json::from_str(&current).unwrap_or_default();
    tags.retain(|t| t != tag);
    let new_json = serde_json::to_string(&tags).map_err(|e| format!("serialize: {}", e))?;
    conn.execute(
        "UPDATE mem_items SET tags_json = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![&new_json, chrono::Utc::now().to_rfc3339(), item_id],
    )
    .map_err(|e| format!("update tags: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
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
        let result = std::panic::catch_unwind(|| codeterm_mem_store::skill_folders::skill_id_for_title(long_chinese));
        assert!(result.is_ok(), "should not panic on non-ASCII skill names");
    }

    #[test]
    fn build_skill_id_emoji_does_not_panic() {
        let emoji_title = "🚀🎯🔥🌟💡🎨🛡️🔑🌈✨🎭🎪🎡🎢🎠🎟🎃🎄🎆🎇"; // many emoji
        let result = std::panic::catch_unwind(|| codeterm_mem_store::skill_folders::skill_id_for_title(emoji_title));
        assert!(result.is_ok(), "should not panic on emoji skill names");
    }
}
