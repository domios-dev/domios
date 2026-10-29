// `mem run-worker` is standalone SQLite maintenance; other backends run through the daemon.
// It exits after one pass; supervisor runs it to isolate maintenance failures.

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
            let mem_dir = db_path.parent().unwrap_or(std::path::Path::new("."));
            let data_dir = if mem_dir.file_name().is_some_and(|name| name == "mem") {
                mem_dir.parent().unwrap_or(mem_dir)
            } else {
                mem_dir
            };
            let skills_root = data_dir.join("skills");
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
    Ok(crate::data_root().join("mem").join("knowledge.db"))
}

/// Open a knowledge.db connection, set busy_timeout, run codeterm-mem-store
/// migrations, and apply the worker's PRAGMAs. Migrations are idempotent;
/// running them on every worker invocation is the cheap way to guarantee
/// the schema matches the binary even when the DB is an older backup.
pub(crate) fn open_worker_db(db_path: &std::path::Path) -> Result<rusqlite::Connection, String> {
    validate_worker_sqlite_path(db_path).map_err(|e| e.to_string())?;
    let conn = rusqlite::Connection::open(db_path)
        .map_err(|e| format!("open {}: {}", db_path.display(), e))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| format!("busy_timeout: {}", e))?;
    codeterm_mem_store::migrations::run(&conn).map_err(|e| format!("migrations: {}", e))?;
    conn.execute_batch("PRAGMA synchronous = NORMAL;")
        .map_err(|e| format!("synchronous pragma: {}", e))?;
    Ok(conn)
}

#[derive(Debug)]
enum WorkerPathError {
    SqlitePathRequired,
}
impl std::fmt::Display for WorkerPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "sqlite_path_required: mem run-worker accepts only a local SQLite database path",
        )
    }
}
fn validate_worker_sqlite_path(path: &std::path::Path) -> Result<(), WorkerPathError> {
    if path.to_string_lossy().contains("://") {
        Err(WorkerPathError::SqlitePathRequired)
    } else {
        Ok(())
    }
}

pub(crate) fn register_sqlite_vec_extension() {
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

use codeterm_mem_store::maintenance::{
    worker_dreaming_deep, worker_dreaming_normalize, WorkerDreamReport,
};
fn worker_dreaming_light(conn: &rusqlite::Connection) -> Result<WorkerDreamReport, String> {
    codeterm_mem_store::maintenance::worker_dreaming_light(conn, &local_machine_id())
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
mod tests;
