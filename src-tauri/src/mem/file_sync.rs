use std::path::Path;
use std::sync::Arc;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::MemManager;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileSyncResult {
    pub total_tracked: usize,
    pub unchanged: usize,
    pub re_indexed: usize,
    pub source_missing: usize,
    pub source_recovered: usize,
    pub errors: Vec<String>,
}

struct TrackedItem {
    id: String,
    source_path: String,
    source_size: Option<i64>,
    source_mtime: Option<String>,
    tags_json: String,
}

pub fn sync_all(mem: &Arc<MemManager>) -> Result<FileSyncResult, String> {
    let mut result = FileSyncResult::default();
    let items = {
        let conn = mem.conn().lock();
        list_tracked(&conn)?
    };
    result.total_tracked = items.len();
    for item in &items {
        let action = {
            let conn = mem.conn().lock();
            sync_one(&conn, item).map(|(action, body_for_embed)| {
                if let Some(body) = body_for_embed {
                    drop(conn);
                    mem.queue_embed(&item.id, &body);
                }
                action
            })
        };
        match action {
            Ok(a) => apply_action(&mut result, a),
            Err(e) => result.errors.push(format!("{}: {}", item.id, e)),
        }
    }
    log_summary(&result);
    Ok(result)
}

pub fn sync_path(mem: &Arc<MemManager>, path: &str) -> Result<FileSyncResult, String> {
    let items: Vec<TrackedItem> = {
        let conn = mem.conn().lock();
        list_tracked(&conn)?
            .into_iter()
            .filter(|i| i.source_path == path)
            .collect()
    };
    if items.is_empty() {
        return Err(format!("no tracked item with source_path={}", path));
    }
    let mut result = FileSyncResult {
        total_tracked: items.len(),
        ..Default::default()
    };
    for item in &items {
        let action = {
            let conn = mem.conn().lock();
            sync_one(&conn, item).map(|(action, body_for_embed)| {
                if let Some(body) = body_for_embed {
                    drop(conn);
                    mem.queue_embed(&item.id, &body);
                }
                action
            })
        };
        match action {
            Ok(a) => apply_action(&mut result, a),
            Err(e) => result.errors.push(format!("{}: {}", item.id, e)),
        }
    }
    Ok(result)
}

/// Sync version: runs the full librarian file-sync pass against a directly
/// owned connection. Used by the `mem run-worker librarian-once` subprocess.
/// When a file's body changes we delete its row in `mem_vectors` so the
/// daemon's `reindex_missing` pass picks it up next — we can't enqueue an
/// embed job here because there is no MemManager / embed_queue.
pub fn sync_all_sync(conn: &Connection) -> Result<FileSyncResult, String> {
    let items = list_tracked(conn)?;
    let mut result = FileSyncResult {
        total_tracked: items.len(),
        ..Default::default()
    };
    for item in &items {
        match sync_one(conn, item) {
            Ok((action, body_for_embed)) => {
                if body_for_embed.is_some() {
                    let _ = conn.execute(
                        "DELETE FROM mem_vectors WHERE mem_id = ?1",
                        rusqlite::params![&item.id],
                    );
                }
                apply_action(&mut result, action);
            }
            Err(e) => result.errors.push(format!("{}: {}", item.id, e)),
        }
    }
    log_summary(&result);
    Ok(result)
}

#[derive(Debug, Clone, Copy)]
enum SyncAction {
    Unchanged,
    ReIndexed,
    Missing,
    Recovered,
}

fn apply_action(result: &mut FileSyncResult, action: SyncAction) {
    match action {
        SyncAction::Unchanged => result.unchanged += 1,
        SyncAction::ReIndexed => result.re_indexed += 1,
        SyncAction::Missing => result.source_missing += 1,
        SyncAction::Recovered => result.source_recovered += 1,
    }
}

fn log_summary(result: &FileSyncResult) {
    log::info!(
        "[file-sync] tracked={} unchanged={} re-indexed={} missing={} recovered={} errors={}",
        result.total_tracked,
        result.unchanged,
        result.re_indexed,
        result.source_missing,
        result.source_recovered,
        result.errors.len(),
    );
}

fn list_tracked(conn: &Connection) -> Result<Vec<TrackedItem>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, source_path, source_size, source_mtime, tags_json \
             FROM mem_items WHERE source_path IS NOT NULL",
        )
        .map_err(|e| format!("prepare: {}", e))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(TrackedItem {
                id: row.get(0)?,
                source_path: row.get(1)?,
                source_size: row.get(2)?,
                source_mtime: row.get(3)?,
                tags_json: row.get(4)?,
            })
        })
        .map_err(|e| format!("query: {}", e))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("collect: {}", e))
}

fn has_tag(tags_json: &str, tag: &str) -> bool {
    serde_json::from_str::<Vec<String>>(tags_json)
        .unwrap_or_default()
        .iter()
        .any(|t| t == tag)
}

/// Returns the action plus the new body content if the file was re-indexed
/// (so the caller can enqueue an embed or delete the stale vector row).
fn sync_one(conn: &Connection, item: &TrackedItem) -> Result<(SyncAction, Option<String>), String> {
    let raw_path = item.source_path.strip_prefix("file:").unwrap_or(&item.source_path);
    let path = Path::new(raw_path);
    let has_missing_tag = has_tag(&item.tags_json, "source-missing");

    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => {
            if !has_missing_tag {
                add_tag(conn, &item.id, "source-missing")?;
            }
            return Ok((SyncAction::Missing, None));
        }
    };

    let current_size = meta.len() as i64;
    let current_mtime = meta
        .modified()
        .ok()
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());

    let size_match = item.source_size.map_or(false, |s| s == current_size);
    let mtime_match = match (&item.source_mtime, &current_mtime) {
        (Some(stored), Some(current)) => stored == current,
        _ => false,
    };

    if size_match && mtime_match && !has_missing_tag {
        return Ok((SyncAction::Unchanged, None));
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("read {}: {}", item.source_path, e))?;

    conn.execute(
        "UPDATE mem_items SET body_inline = ?1, body_blob_path = NULL, \
         source_size = ?2, source_mtime = ?3, updated_at = ?4 \
         WHERE id = ?5",
        rusqlite::params![
            &content,
            current_size,
            &current_mtime,
            chrono::Utc::now().to_rfc3339(),
            &item.id,
        ],
    )
    .map_err(|e| format!("update: {}", e))?;

    conn.execute(
        "UPDATE mem_fts SET body = ?1 WHERE mem_id = ?2",
        rusqlite::params![&content, &item.id],
    )
    .map_err(|e| format!("update fts: {}", e))?;

    if has_missing_tag {
        remove_tag(conn, &item.id, "source-missing")?;
        return Ok((SyncAction::Recovered, Some(content)));
    }

    Ok((SyncAction::ReIndexed, Some(content)))
}

fn add_tag(conn: &Connection, item_id: &str, tag: &str) -> Result<(), String> {
    let current: String = conn
        .query_row(
            "SELECT tags_json FROM mem_items WHERE id = ?1",
            [item_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("read tags: {}", e))?;
    let mut tags: Vec<String> = serde_json::from_str(&current).unwrap_or_default();
    if !tags.iter().any(|t| t == tag) {
        tags.push(tag.to_string());
        let new_json =
            serde_json::to_string(&tags).map_err(|e| format!("serialize tags: {}", e))?;
        conn.execute(
            "UPDATE mem_items SET tags_json = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![&new_json, chrono::Utc::now().to_rfc3339(), item_id],
        )
        .map_err(|e| format!("update tags: {}", e))?;
    }
    Ok(())
}

fn remove_tag(conn: &Connection, item_id: &str, tag: &str) -> Result<(), String> {
    let current: String = conn
        .query_row(
            "SELECT tags_json FROM mem_items WHERE id = ?1",
            [item_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("read tags: {}", e))?;
    let mut tags: Vec<String> = serde_json::from_str(&current).unwrap_or_default();
    tags.retain(|t| t != tag);
    let new_json =
        serde_json::to_string(&tags).map_err(|e| format!("serialize tags: {}", e))?;
    conn.execute(
        "UPDATE mem_items SET tags_json = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![&new_json, chrono::Utc::now().to_rfc3339(), item_id],
    )
    .map_err(|e| format!("update tags: {}", e))?;
    Ok(())
}
