use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use super::blob::{read_blob, write_blob};
use super::{
    history_log, HistorySnapshot, MemItem, MemUpdate, NewMemItem, INLINE_BODY_LIMIT,
};
use crate::mem::ids;

/// >2KB bodies go to content-addressed blob.
pub fn insert(
    conn: &Connection,
    item: &NewMemItem,
    blobs_dir: &Path,
) -> Result<String, String> {
    insert_with_options(conn, item, blobs_dir, false)
}

pub fn upsert(
    conn: &Connection,
    item: &NewMemItem,
    blobs_dir: &Path,
) -> Result<String, String> {
    insert_with_options(conn, item, blobs_dir, true)
}

fn insert_with_options(
    conn: &Connection,
    item: &NewMemItem,
    blobs_dir: &Path,
    replace: bool,
) -> Result<String, String> {
    let kind_str = item.kind.as_str();
    let slug = ids::slugify(&item.title);
    let candidate = ids::compose(kind_str, &slug);

    let collision = exists(conn, &candidate).map_err(|e| e.to_string())?;
    let id = if collision && replace {
        // Capture the prior state for history before the row disappears.
        let prior: Option<(String, Option<String>, f64, String)> = conn
            .query_row(
                r#"SELECT title, body_inline, importance, kind
                   FROM mem_items WHERE id = ?1"#,
                params![&candidate],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(|e| format!("pre-replace fetch: {}", e))?;
        conn.execute("DELETE FROM mem_items WHERE id = ?1", params![&candidate])
            .map_err(|e| format!("delete on replace: {}", e))?;
        let _ = conn.execute("DELETE FROM mem_fts WHERE mem_id = ?1", params![&candidate]);
        if let Some((old_title, old_body, old_imp, old_kind)) = prior {
            history_log(
                conn,
                &candidate,
                "delete",
                &HistorySnapshot {
                    title: Some(old_title),
                    body: old_body,
                    importance: Some(old_imp),
                    kind: Some(old_kind),
                },
                &HistorySnapshot::default(),
                Some("replaced_by_upsert"),
            );
        }
        candidate
    } else if collision {
        ids::with_collision_suffix(kind_str, &slug, &item.body)
    } else {
        candidate
    };

    let uid = ulid::Ulid::new().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let scope_str = item.scope.as_str();
    let tags_json = serde_json::to_string(&item.tags)
        .map_err(|e| format!("serialize tags: {}", e))?;

    let (body_inline, body_blob_path): (Option<String>, Option<String>) =
        if item.body.len() > INLINE_BODY_LIMIT {
            let path = write_blob(blobs_dir, &item.body)?;
            (None, Some(path))
        } else {
            (Some(item.body.clone()), None)
        };

    let (source_size, source_mtime) = item
        .source_path
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| {
            let size = m.len() as i64;
            let mtime = m
                .modified()
                .ok()
                .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
            (Some(size), mtime)
        })
        .unwrap_or((None, None));

    conn.execute(
        r#"INSERT INTO mem_items
           (id, uid, kind, scope, title, summary, area, tags_json,
            body_inline, body_blob_path, source, importance,
            created_at, updated_at, accessed_at,
            source_path, source_size, source_mtime)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, NULL, ?15, ?16, ?17)"#,
        params![
            &id,
            &uid,
            kind_str,
            &scope_str,
            &item.title,
            &item.summary,
            &item.area,
            &tags_json,
            &body_inline,
            &body_blob_path,
            &item.source,
            item.importance,
            &now,
            &now,
            &item.source_path,
            source_size,
            &source_mtime,
        ],
    )
    .map_err(|e| format!("insert mem_item: {}", e))?;

    let tags_text = item.tags.join(" ");
    conn.execute(
        r#"INSERT INTO mem_fts (mem_id, title, summary, body, tags)
           VALUES (?1, ?2, ?3, ?4, ?5)"#,
        params![
            &id,
            &item.title,
            item.summary.as_deref().unwrap_or(""),
            &item.body,
            &tags_text,
        ],
    )
    .map_err(|e| format!("insert fts: {}", e))?;

    history_log(
        conn,
        &id,
        "create",
        &HistorySnapshot::default(),
        &HistorySnapshot {
            title: Some(item.title.clone()),
            body: Some(item.body.clone()),
            importance: Some(item.importance),
            kind: Some(kind_str.to_string()),
        },
        None,
    );

    Ok(id)
}

pub fn exists(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM mem_items WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

pub fn get(
    conn: &Connection,
    id: &str,
    blobs_dir: &Path,
) -> Result<Option<MemItem>, String> {
    let row: Option<(String, String, String, String, String, Option<String>, Option<String>, String, Option<String>, Option<String>, Option<String>, f64, String, String, Option<String>, i64, i64)> = conn
        .query_row(
            r#"SELECT id, uid, kind, scope, title, summary, area, tags_json,
                      body_inline, body_blob_path, source, importance,
                      created_at, updated_at, accessed_at, access_count, recurrence_count
               FROM mem_items WHERE id = ?1"#,
            params![id],
            |r| {
                Ok((
                    r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?,
                    r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?,
                    r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?, r.get(14)?,
                    r.get(15)?, r.get(16)?,
                ))
            },
        )
        .optional()
        .map_err(|e| format!("query mem_item: {}", e))?;

    let Some((id, uid, kind, scope, title, summary, area, tags_json,
              body_inline, body_blob_path, source, importance,
              created_at, updated_at, accessed_at, access_count, recurrence_count)) = row else {
        return Ok(None);
    };

    let mut body = if let Some(inline) = body_inline {
        inline
    } else if let Some(rel) = body_blob_path.as_ref() {
        read_blob(blobs_dir, rel)?
    } else {
        String::new()
    };

    if let Some(ref src) = source {
        if let Some(path) = src.strip_prefix("file:") {
            if let Ok(disk_content) = std::fs::read_to_string(path) {
                let trimmed = disk_content.trim();
                if !trimmed.is_empty() {
                    body = trimmed.to_string();
                }
            }
        }
    }

    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();

    let now = chrono::Utc::now().to_rfc3339();
    let touched = conn.execute(
        "UPDATE mem_items SET accessed_at = ?1, access_count = access_count + 1 WHERE id = ?2",
        params![&now, &id],
    );
    let (new_accessed_at, new_access_count) = match touched {
        Ok(n) if n > 0 => (Some(now.clone()), access_count + 1),
        _ => (accessed_at, access_count),
    };

    Ok(Some(MemItem {
        id, uid, kind, scope, title, summary, area, tags, body, source,
        importance, created_at, updated_at,
        accessed_at: new_accessed_at,
        access_count: new_access_count,
        recurrence_count,
    }))
}

pub fn update(
    conn: &Connection,
    id: &str,
    patch: &MemUpdate,
    blobs_dir: &Path,
) -> Result<bool, String> {
    let existing = get(conn, id, blobs_dir)?;
    let Some(prev) = existing else {
        return Ok(false);
    };

    let prev_title = prev.title.clone();
    let prev_body = prev.body.clone();
    let prev_importance = prev.importance;
    let prev_kind = prev.kind.clone();

    let new_title = patch.title.clone().unwrap_or(prev.title);
    let new_summary = patch.summary.clone().or(prev.summary);
    let new_area = patch.area.clone().or(prev.area);
    let new_tags = patch.tags.clone().unwrap_or(prev.tags);
    let new_body = patch.body.clone().unwrap_or(prev.body);
    let new_importance = patch.importance.unwrap_or(prev.importance);

    let tags_json = serde_json::to_string(&new_tags)
        .map_err(|e| format!("serialize tags: {}", e))?;
    let now = chrono::Utc::now().to_rfc3339();

    let (body_inline, body_blob_path): (Option<String>, Option<String>) =
        if new_body.len() > INLINE_BODY_LIMIT {
            let path = write_blob(blobs_dir, &new_body)?;
            (None, Some(path))
        } else {
            (Some(new_body.clone()), None)
        };

    let recur_bump = patch.bump_recurrence.unwrap_or(0);
    conn.execute(
        r#"UPDATE mem_items
           SET title = ?1, summary = ?2, area = ?3, tags_json = ?4,
               body_inline = ?5, body_blob_path = ?6, importance = ?7,
               updated_at = ?8, recurrence_count = recurrence_count + ?10
           WHERE id = ?9"#,
        params![
            &new_title, &new_summary, &new_area, &tags_json,
            &body_inline, &body_blob_path, new_importance, &now, &id, recur_bump,
        ],
    ).map_err(|e| format!("update mem_item: {}", e))?;

    let tags_text = new_tags.join(" ");
    let _ = conn.execute("DELETE FROM mem_fts WHERE mem_id = ?1", params![&id]);
    conn.execute(
        r#"INSERT INTO mem_fts (mem_id, title, summary, body, tags)
           VALUES (?1, ?2, ?3, ?4, ?5)"#,
        params![
            &id, &new_title, new_summary.as_deref().unwrap_or(""), &new_body, &tags_text,
        ],
    ).map_err(|e| format!("update fts: {}", e))?;

    // Record only the fields that actually changed. If nothing changed
    // (e.g. patch touched only tags/area/summary), still emit an update row
    // so the caller can see the write happened — but leave old_/new_ at
    // None for fields that were unchanged.
    let mut old = HistorySnapshot::default();
    let mut new = HistorySnapshot::default();
    let mut dirty = false;
    if new_title != prev_title {
        old.title = Some(prev_title);
        new.title = Some(new_title);
        dirty = true;
    }
    if new_body != prev_body {
        old.body = Some(prev_body);
        new.body = Some(new_body);
        dirty = true;
    }
    if (new_importance - prev_importance).abs() > f64::EPSILON {
        old.importance = Some(prev_importance);
        new.importance = Some(new_importance);
        dirty = true;
    }
    // Kind is not patchable via MemUpdate, but record it so downstream
    // consumers can see which kind the item was at update time.
    if dirty {
        old.kind = Some(prev_kind.clone());
        new.kind = Some(prev_kind);
        history_log(conn, id, "update", &old, &new, None);
    }

    Ok(true)
}

pub fn delete(conn: &Connection, id: &str) -> Result<bool, String> {
    delete_with_reason(conn, id, None)
}

/// Delete with an optional reason — used by the dedup/merge path to record
/// `merge_into=<winner_id>` in the history row.
pub fn delete_with_reason(
    conn: &Connection,
    id: &str,
    reason: Option<&str>,
) -> Result<bool, String> {
    // Capture prior state for history BEFORE removing the row.
    let prior: Option<(String, Option<String>, f64, String)> = conn
        .query_row(
            r#"SELECT title, body_inline, importance, kind
               FROM mem_items WHERE id = ?1"#,
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(|e| format!("pre-delete fetch: {}", e))?;

    let n = conn
        .execute("DELETE FROM mem_items WHERE id = ?1", params![id])
        .map_err(|e| format!("delete mem_item: {}", e))?;
    let _ = conn.execute("DELETE FROM mem_fts WHERE mem_id = ?1", params![id]);
    let _ = conn.execute(
        "DELETE FROM mem_vectors WHERE mem_id = ?1 OR mem_id LIKE ?2",
        params![id, format!("{}_chunk_%", id)],
    );
    let _ = conn.execute(
        "DELETE FROM relations WHERE from_id = ?1 OR to_id = ?1",
        params![id],
    );

    if n > 0 {
        if let Some((old_title, old_body, old_imp, old_kind)) = prior {
            let change_type = if reason.map_or(false, |r| r.starts_with("merge_into=")) {
                "merge"
            } else {
                "delete"
            };
            history_log(
                conn,
                id,
                change_type,
                &HistorySnapshot {
                    title: Some(old_title),
                    body: old_body,
                    importance: Some(old_imp),
                    kind: Some(old_kind),
                },
                &HistorySnapshot::default(),
                reason,
            );
        }
    }
    Ok(n > 0)
}
