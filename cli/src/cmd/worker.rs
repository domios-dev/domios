// `mem run-worker` subprocess.
//
// Opens knowledge.db directly (multi-process WAL) and runs one phase of
// dreaming or the librarian inline, then exits. The supervisor in the
// daemon spawns this so a panic / OOM / hang in the maintenance pass
// cannot take the API down.
//
// SQL is duplicated from src-tauri/src/mem/dreaming.rs / file_sync.rs on
// purpose — keeping the CLI free of the codeterm_lib dependency means it
// stays standalone and lightweight. The dreaming algorithm is stable;
// changes need to be applied here too.

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
        }
    };

    register_sqlite_vec_extension();

    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            worker_emit_error(&format!("open {}: {}", db_path.display(), e));
            std::process::exit(1);
        }
    };
    if let Err(e) = conn.busy_timeout(std::time::Duration::from_secs(5)) {
        worker_emit_error(&format!("busy_timeout: {}", e));
        std::process::exit(1);
    }
    if let Err(e) = conn.execute_batch("PRAGMA synchronous = NORMAL;") {
        worker_emit_error(&format!("synchronous pragma: {}", e));
        std::process::exit(1);
    }

    let result = match kind {
        "dreaming-light" => worker_dreaming_light(&conn).map(|r| serde_json::json!({
            "considered": r.considered, "promoted": r.promoted,
            "dropped": r.dropped, "phase": r.phase,
        })),
        "dreaming-deep" => worker_dreaming_deep(&conn).map(|r| serde_json::json!({
            "considered": r.considered, "promoted": r.promoted,
            "dropped": r.dropped, "phase": r.phase,
        })),
        "librarian-once" => {
            let skills_root = dirs::home_dir().unwrap_or_default()
                .join(data_dir_name()).join("skills");
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

fn register_sqlite_vec_extension() {
    use std::sync::Once;
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        }
    });
}

#[derive(Debug, Clone)]
struct WorkerDreamReport {
    considered: u64,
    promoted: u64,
    dropped: u64,
    phase: String,
}

fn worker_dreaming_light(conn: &rusqlite::Connection) -> Result<WorkerDreamReport, String> {
    let now_ts = chrono::Utc::now();
    let one_day_ago = (now_ts - chrono::Duration::days(1)).to_rfc3339();

    let candidates: Vec<(String, i64, f64, String, Option<String>)> = {
        let mut stmt = conn.prepare(
            r#"SELECT id, access_count, importance, created_at, accessed_at
               FROM mem_items
               WHERE kind = 'learning'
                 AND access_count >= 3
                 AND created_at < ?1"#,
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(rusqlite::params![one_day_ago], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        }).map_err(|e| e.to_string())?;
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
                let days = (now_ts - a.with_timezone(&chrono::Utc))
                    .num_hours().max(0) as f64 / 24.0;
                (-days * 0.693 / 14.0).exp()
            })
            .unwrap_or(0.0);
        let imp_score = (*importance / 5.0).min(1.0);
        let age = chrono::DateTime::parse_from_rfc3339(created_at)
            .ok()
            .map(|c| {
                let days = (now_ts - c.with_timezone(&chrono::Utc))
                    .num_hours().max(0) as f64 / 24.0;
                (days / 7.0).min(1.0)
            })
            .unwrap_or(0.5);

        let score = frequency * 0.30 + recency * 0.25 + imp_score * 0.25 + age * 0.20;
        if score < 0.5 { continue; }

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

    Ok(WorkerDreamReport {
        considered, promoted, dropped: 0,
        phase: "light".into(),
    })
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
        let n = conn.execute(
            "UPDATE mem_items SET importance = 0.0, updated_at = ?1 \
             WHERE kind = ?2 AND access_count = 0 AND created_at < ?3 \
             AND importance > 0.0 AND importance < 4.0",
            rusqlite::params![&now, kind, &cutoff],
        ).map_err(|e| e.to_string())? as u64;
        total_expired += n;
    }
    Ok(WorkerDreamReport {
        considered: total_expired,
        promoted: 0,
        dropped: total_expired,
        phase: "deep".into(),
    })
}

#[derive(Debug, Default, serde::Serialize)]
struct WorkerLibrarianReport {
    skill_folders_registered: usize,
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
    librarian_scan_skill_folders(conn, skills_root, &mut report);
    librarian_sync_files(conn, &mut report)?;
    Ok(report)
}

fn librarian_scan_skill_folders(
    conn: &rusqlite::Connection,
    skills_root: &std::path::Path,
    report: &mut WorkerLibrarianReport,
) {
    if !skills_root.is_dir() { return }
    let mut known: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT source_path FROM mem_items WHERE source_path LIKE 'file:%SKILL.md'"
    ) {
        if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
            for row in rows.flatten() { known.insert(row); }
        }
    }

    let entries = match std::fs::read_dir(skills_root) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let folder = entry.path();
        if !folder.is_dir() { continue; }
        let skill_md = folder.join("SKILL.md");
        if !skill_md.is_file() { continue; }
        let source = format!("file:{}", skill_md.display());
        if known.contains(&source) { continue; }

        let body = match std::fs::read_to_string(&skill_md) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let title = title_from_skill_md(&body)
            .unwrap_or_else(|| folder.file_name().map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "skill".into()));
        let area = area_from_skill_md(&body);

        let id = build_skill_id(&title);
        let now = chrono::Utc::now().to_rfc3339();
        let tags_json = serde_json::to_string(&vec!["folder-skill", "auto-registered"]).unwrap_or("[]".into());
        let result = conn.execute(
            r#"INSERT OR IGNORE INTO mem_items
               (id, uid, kind, scope, title, summary, area, tags_json,
                body_inline, source, importance, created_at, updated_at,
                source_path)
               VALUES (?1, ?2, 'skill', 'global', ?3, NULL, ?4, ?5,
                       ?6, ?7, 4.0, ?8, ?8, ?7)"#,
            rusqlite::params![
                &id, &uuid::Uuid::new_v4().to_string(),
                &title, &area, &tags_json, &body, &source, &now,
            ],
        );
        match result {
            Ok(_n) => {
                let _ = conn.execute(
                    "INSERT OR REPLACE INTO mem_fts (mem_id, title, summary, body, tags) \
                     VALUES (?1, ?2, '', ?3, ?4)",
                    rusqlite::params![&id, &title, &body, &tags_json],
                );
                report.skill_folders_registered += 1;
            }
            Err(e) => report.errors.push(format!("register skill {}: {}", source, e)),
        }
    }
}

fn build_skill_id(title: &str) -> String {
    let slug: String = title.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    let trimmed = slug.trim_matches('_');
    let short = if trimmed.len() > 40 { &trimmed[..40] } else { trimmed };
    format!("skill_{}", short)
}

fn title_from_skill_md(body: &str) -> Option<String> {
    if let Some(line) = body.lines().take(20).find(|l| l.trim_start().starts_with("title:")) {
        let v = line.trim_start().trim_start_matches("title:").trim().trim_matches('"').to_string();
        if !v.is_empty() { return Some(v); }
    }
    body.lines().find(|l| l.trim_start().starts_with("# "))
        .map(|l| l.trim_start().trim_start_matches("# ").trim().to_string())
}

fn area_from_skill_md(body: &str) -> Option<String> {
    body.lines().take(20)
        .find(|l| l.trim_start().starts_with("area:"))
        .map(|l| l.trim_start().trim_start_matches("area:").trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
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
        let mut stmt = conn.prepare(
            "SELECT id, source_path, source_size, source_mtime, tags_json \
             FROM mem_items WHERE source_path IS NOT NULL"
        ).map_err(|e| format!("prepare: {}", e))?;
        let rows = stmt.query_map([], |row| {
            Ok(LibrarianTrackedItem {
                id: row.get(0)?,
                source_path: row.get(1)?,
                source_size: row.get(2)?,
                source_mtime: row.get(3)?,
                tags_json: row.get(4)?,
            })
        }).map_err(|e| format!("query: {}", e))?;
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
    let raw_path = item.source_path.strip_prefix("file:").unwrap_or(&item.source_path);
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
    let current_mtime = meta.modified().ok()
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
    let size_match = item.source_size.map_or(false, |s| s == current_size);
    let mtime_match = match (&item.source_mtime, &current_mtime) {
        (Some(stored), Some(current)) => stored == current,
        _ => false,
    };
    if size_match && mtime_match && !has_missing_tag {
        return Ok(LibrarianSyncAction::Unchanged);
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("read {}: {}", item.source_path, e))?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE mem_items SET body_inline = ?1, body_blob_path = NULL, \
         source_size = ?2, source_mtime = ?3, updated_at = ?4 \
         WHERE id = ?5",
        rusqlite::params![&content, current_size, &current_mtime, &now, &item.id],
    ).map_err(|e| format!("update: {}", e))?;
    conn.execute(
        "UPDATE mem_fts SET body = ?1 WHERE mem_id = ?2",
        rusqlite::params![&content, &item.id],
    ).map_err(|e| format!("update fts: {}", e))?;
    // Drop any stale embedding so the daemon's reindex_missing pass picks
    // it up on the next startup tick. Done via vec0 virtual table — the
    // sqlite-vec auto-extension was registered before Connection::open.
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
    let current: String = conn.query_row(
        "SELECT tags_json FROM mem_items WHERE id = ?1",
        [item_id], |r| r.get(0),
    ).map_err(|e| format!("read tags: {}", e))?;
    let mut tags: Vec<String> = serde_json::from_str(&current).unwrap_or_default();
    if !tags.iter().any(|t| t == tag) {
        tags.push(tag.to_string());
        let new_json = serde_json::to_string(&tags).map_err(|e| format!("serialize: {}", e))?;
        conn.execute(
            "UPDATE mem_items SET tags_json = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![&new_json, chrono::Utc::now().to_rfc3339(), item_id],
        ).map_err(|e| format!("update tags: {}", e))?;
    }
    Ok(())
}

fn librarian_remove_tag(conn: &rusqlite::Connection, item_id: &str, tag: &str) -> Result<(), String> {
    let current: String = conn.query_row(
        "SELECT tags_json FROM mem_items WHERE id = ?1",
        [item_id], |r| r.get(0),
    ).map_err(|e| format!("read tags: {}", e))?;
    let mut tags: Vec<String> = serde_json::from_str(&current).unwrap_or_default();
    tags.retain(|t| t != tag);
    let new_json = serde_json::to_string(&tags).map_err(|e| format!("serialize: {}", e))?;
    conn.execute(
        "UPDATE mem_items SET tags_json = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![&new_json, chrono::Utc::now().to_rfc3339(), item_id],
    ).map_err(|e| format!("update tags: {}", e))?;
    Ok(())
}
