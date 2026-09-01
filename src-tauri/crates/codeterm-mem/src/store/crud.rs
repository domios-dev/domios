use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use super::blob::{read_blob, write_blob};
use super::{
    history_log, HistorySnapshot, MemItem, MemKind, MemScope, MemUpdate, NewMemItem,
    INLINE_BODY_LIMIT,
};
use crate::ids;

/// >2KB bodies go to content-addressed blob.
pub fn insert(conn: &Connection, item: &NewMemItem, blobs_dir: &Path) -> Result<String, String> {
    insert_with_options(conn, item, blobs_dir, false, None)
}

pub fn upsert(conn: &Connection, item: &NewMemItem, blobs_dir: &Path) -> Result<String, String> {
    insert_with_options(conn, item, blobs_dir, true, None)
}

/// Create a bundled item and record its fingerprint in the SAME insert —
/// never a two-step create-then-bootstrap with an unfingerprinted window.
pub fn insert_seeded(
    conn: &Connection,
    item: &NewMemItem,
    blobs_dir: &Path,
    fingerprint: &str,
) -> Result<String, String> {
    insert_with_options(conn, item, blobs_dir, false, Some(fingerprint))
}

fn insert_with_options(
    conn: &Connection,
    item: &NewMemItem,
    blobs_dir: &Path,
    replace: bool,
    fingerprint: Option<&str>,
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
    let tags_json =
        serde_json::to_string(&item.tags).map_err(|e| format!("serialize tags: {}", e))?;

    let (body_inline, body_blob_path): (Option<String>, Option<String>) =
        if item.body.len() > INLINE_BODY_LIMIT {
            let path = write_blob(blobs_dir, &item.body)?;
            (None, Some(path))
        } else {
            (Some(item.body.clone()), None)
        };

    // call — both here and in mem/file_sync.rs::sync_one.
    let (source_size, source_mtime) = item
        .source_path
        .as_ref()
        .map(|p| p.strip_prefix("file:").unwrap_or(p))
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

    let pinned_int: i64 = if item.pinned { 1 } else { 0 };
    conn.execute(
        r#"INSERT INTO mem_items
           (id, uid, kind, scope, title, summary, area, tags_json,
            body_inline, body_blob_path, source, importance,
            created_at, updated_at, accessed_at,
            source_path, source_size, source_mtime, is_pinned_wakeup, seed_fingerprint)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, NULL, ?15, ?16, ?17, ?18, ?19)"#,
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
            pinned_int,
            fingerprint,
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

pub fn get(conn: &Connection, id: &str, blobs_dir: &Path) -> Result<Option<MemItem>, String> {
    let row: Option<(
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        f64,
        String,
        String,
        Option<String>,
        i64,
        i64,
        Option<i64>,
        Option<String>,
    )> = conn
        .query_row(
            r#"SELECT id, uid, kind, scope, title, summary, area, tags_json,
                      body_inline, body_blob_path, source, importance,
                      created_at, updated_at, accessed_at, access_count, recurrence_count,
                      source_size, source_mtime
               FROM mem_items WHERE id = ?1"#,
            params![id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                    r.get(11)?,
                    r.get(12)?,
                    r.get(13)?,
                    r.get(14)?,
                    r.get(15)?,
                    r.get(16)?,
                    r.get(17)?,
                    r.get(18)?,
                ))
            },
        )
        .optional()
        .map_err(|e| format!("query mem_item: {}", e))?;

    let Some((
        id,
        uid,
        kind,
        scope,
        title,
        summary,
        area,
        tags_json,
        body_inline,
        body_blob_path,
        source,
        importance,
        created_at,
        mut updated_at,
        accessed_at,
        access_count,
        recurrence_count,
        stored_source_size,
        stored_source_mtime,
    )) = row
    else {
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
            // Skip redundant stat+UPDATE when row was just touched recently; opportunistic refresh only.
            let recently_accessed = accessed_at
                .as_ref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|t| (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds() < 10)
                .unwrap_or(false);
            if !recently_accessed {
                if let Ok(meta) = std::fs::metadata(path) {
                    let current_size = meta.len() as i64;
                    let current_mtime = meta
                        .modified()
                        .ok()
                        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
                    let size_diff = stored_source_size != Some(current_size);
                    let mtime_diff = stored_source_mtime != current_mtime;
                    if size_diff || mtime_diff {
                        let bumped = chrono::Utc::now().to_rfc3339();
                        if conn.execute(
                            "UPDATE mem_items SET source_size = ?1, source_mtime = ?2, updated_at = ?3 \
                             WHERE id = ?4",
                            params![current_size, &current_mtime, &bumped, &id],
                        ).is_ok() {
                            updated_at = bumped;
                        }
                    }
                }
            }
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
        id,
        uid,
        kind,
        scope,
        title,
        summary,
        area,
        tags,
        body,
        source,
        importance,
        created_at,
        updated_at,
        accessed_at: new_accessed_at,
        access_count: new_access_count,
        recurrence_count,
    }))
}

/// Every field the bundled seeder manages, plus its recorded fingerprint.
/// A raw read: unlike `get`, never bumps `accessed_at`/`access_count`.
pub fn managed_payload(
    conn: &Connection,
    id: &str,
    blobs_dir: &Path,
) -> Result<Option<(NewMemItem, Option<String>)>, String> {
    #[allow(clippy::type_complexity)]
    let row: Option<(
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        f64,
        Option<String>,
        i64,
        Option<String>,
    )> = conn
        .query_row(
            r#"SELECT kind, scope, title, summary, area, tags_json,
                      body_inline, body_blob_path, source, importance,
                      source_path, COALESCE(is_pinned_wakeup, 0), seed_fingerprint
               FROM mem_items WHERE id = ?1"#,
            params![id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                    r.get(11)?,
                    r.get(12)?,
                ))
            },
        )
        .optional()
        .map_err(|e| format!("query managed payload: {}", e))?;
    let Some((
        kind,
        scope,
        title,
        summary,
        area,
        tags_json,
        body_inline,
        body_blob_path,
        source,
        importance,
        source_path,
        pinned,
        fingerprint,
    )) = row
    else {
        return Ok(None);
    };
    let Some(kind) = MemKind::from_str(&kind) else {
        return Err(format!("unrecognized kind {kind}"));
    };
    // `MemScope::from_str` is infallible (defaults to Global), so an
    // unrecognized stored value must be caught by round-tripping it.
    let parsed_scope = MemScope::from_str(&scope);
    if parsed_scope.as_str() != scope {
        return Err(format!("unrecognized scope {scope}"));
    }
    let body = if let Some(inline) = body_inline {
        inline
    } else if let Some(rel) = body_blob_path.as_ref() {
        read_blob(blobs_dir, rel)?
    } else {
        String::new()
    };
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    Ok(Some((
        NewMemItem {
            kind,
            scope: parsed_scope,
            title,
            summary,
            area,
            tags,
            body,
            source,
            importance,
            source_path,
            pinned: pinned != 0,
        },
        fingerprint,
    )))
}

/// Overwrite a bundled item's managed fields, guarded by an optimistic
/// `seed_fingerprint IS expected` CAS. `Ok(false)` = lost a race, skip it.
pub fn cas_refresh_bundled(
    conn: &Connection,
    id: &str,
    blobs_dir: &Path,
    expected_fingerprint: Option<&str>,
    new_item: &NewMemItem,
    new_fingerprint: &str,
) -> Result<bool, String> {
    let (body_inline, body_blob_path): (Option<String>, Option<String>) =
        if new_item.body.len() > INLINE_BODY_LIMIT {
            (None, Some(write_blob(blobs_dir, &new_item.body)?))
        } else {
            (Some(new_item.body.clone()), None)
        };
    let tags_json =
        serde_json::to_string(&new_item.tags).map_err(|e| format!("serialize tags: {}", e))?;
    let now = chrono::Utc::now().to_rfc3339();
    let pinned_int: i64 = if new_item.pinned { 1 } else { 0 };
    let scope_str = new_item.scope.as_str();
    let affected = conn
        .execute(
            r#"UPDATE mem_items
               SET kind = ?1, scope = ?2, title = ?3, summary = ?4, area = ?5,
                   tags_json = ?6, body_inline = ?7, body_blob_path = ?8,
                   importance = ?9, source_path = ?10, is_pinned_wakeup = ?11,
                   updated_at = ?12, seed_fingerprint = ?13
               WHERE id = ?14 AND seed_fingerprint IS ?15"#,
            params![
                new_item.kind.as_str(),
                &scope_str,
                &new_item.title,
                &new_item.summary,
                &new_item.area,
                &tags_json,
                &body_inline,
                &body_blob_path,
                new_item.importance,
                &new_item.source_path,
                pinned_int,
                &now,
                new_fingerprint,
                &id,
                expected_fingerprint,
            ],
        )
        .map_err(|e| format!("cas refresh bundled: {}", e))?;
    if affected > 0 {
        let _ = conn.execute("DELETE FROM mem_fts WHERE mem_id = ?1", params![&id]);
        let _ = conn.execute(
            r#"INSERT INTO mem_fts (mem_id, title, summary, body, tags)
               VALUES (?1, ?2, ?3, ?4, ?5)"#,
            params![
                &id,
                &new_item.title,
                new_item.summary.as_deref().unwrap_or(""),
                &new_item.body,
                &new_item.tags.join(" "),
            ],
        );
    }
    Ok(affected > 0)
}

/// Re-reads the row and refuses unless it is STILL unfingerprinted AND
/// byte-identical to `expected` — `IS NULL` alone would accept an edit too.
pub fn bootstrap_seed_fingerprint(
    conn: &Connection,
    id: &str,
    blobs_dir: &Path,
    expected: &NewMemItem,
    fingerprint: &str,
) -> Result<bool, String> {
    let Some((current, recorded)) = managed_payload(conn, id, blobs_dir)? else {
        return Ok(false);
    };
    if recorded.is_some() || &current != expected {
        return Ok(false);
    }
    let affected = conn
        .execute(
            "UPDATE mem_items SET seed_fingerprint = ?1 WHERE id = ?2 AND seed_fingerprint IS NULL",
            params![fingerprint, id],
        )
        .map_err(|e| format!("bootstrap seed fingerprint: {}", e))?;
    Ok(affected > 0)
}

/// Retrieve `id` as a MemItem, falling back to a synthesized MemItem from
/// the entities table if there's no mem_item with this id but an entity
/// exists. Used by mem_search so vector recall hits on entity ids surface
/// to the user as proper search results (with kind="entity:Person" / etc).
///
/// The synthesized MemItem has:
///   kind  = "entity:<EntityType>"  (e.g. "entity:Person")
///   title = properties.name when present, else the entity id
///   summary = brief properties summary
///   body = pretty-printed properties_json
///   area = "_graph"
///   importance = 3.0 (default)
pub fn get_or_synthesize_entity(
    conn: &Connection,
    id: &str,
    blobs_dir: &Path,
) -> Result<Option<MemItem>, String> {
    if let Some(item) = get(conn, id, blobs_dir)? {
        return Ok(Some(item));
    }
    let row: Option<(String, String, String, String, String, String)> = conn
        .query_row(
            "SELECT id, entity_type, scope, properties_json, created_at, updated_at
             FROM entities WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .optional()
        .map_err(|e| format!("query entity: {}", e))?;
    let Some((id, entity_type, scope, props_json, created_at, updated_at)) = row else {
        return Ok(None);
    };
    let props_val: serde_json::Value =
        serde_json::from_str(&props_json).unwrap_or(serde_json::Value::Null);
    let title = props_val
        .as_object()
        .and_then(|m| m.get("name"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| id.clone());
    let summary = match &props_val {
        serde_json::Value::Object(m) if !m.is_empty() => Some(
            m.iter()
                .filter(|(k, _)| k.as_str() != "name")
                .map(|(k, v)| {
                    let v_str = match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    format!("{}: {}", k, v_str)
                })
                .collect::<Vec<_>>()
                .join(" · "),
        ),
        _ => None,
    };
    let body = serde_json::to_string_pretty(&props_val).unwrap_or(props_json);
    Ok(Some(MemItem {
        id: id.clone(),
        uid: id,
        kind: format!("entity:{}", entity_type),
        scope,
        title,
        summary,
        area: Some("_graph".to_string()),
        tags: vec!["graph-entity".to_string(), entity_type.to_lowercase()],
        body,
        source: None,
        importance: 3.0,
        created_at,
        updated_at,
        accessed_at: None,
        access_count: 0,
        recurrence_count: 0,
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
    let prev_summary = prev.summary.clone();
    let prev_area = prev.area.clone();
    let prev_tags = prev.tags.clone();
    let (prev_pinned, prev_fingerprint): (bool, Option<String>) = conn
        .query_row(
            "SELECT COALESCE(is_pinned_wakeup, 0), seed_fingerprint FROM mem_items WHERE id = ?1",
            params![id],
            |r| Ok((r.get::<_, i64>(0)? != 0, r.get(1)?)),
        )
        .map_err(|e| format!("read is_pinned_wakeup/seed_fingerprint: {}", e))?;

    let new_title = patch.title.clone().unwrap_or(prev.title);
    let new_summary = patch.summary.clone().or(prev.summary);
    let new_area = patch.area.clone().or(prev.area);
    let new_tags = patch.tags.clone().unwrap_or(prev.tags);
    let new_body = patch.body.clone().unwrap_or(prev.body);
    let new_importance = patch.importance.unwrap_or(prev.importance);
    let new_pinned = patch.pinned.unwrap_or(prev_pinned);

    // A managed-field edit relinquishes bundled ownership atomically in
    // this write; `cas_refresh_bundled` can no longer claim it.
    let managed_changed = new_title != prev_title
        || new_summary != prev_summary
        || new_area != prev_area
        || new_tags != prev_tags
        || new_body != prev_body
        || (new_importance - prev_importance).abs() > f64::EPSILON
        || new_pinned != prev_pinned;
    let new_fingerprint = if managed_changed {
        None
    } else {
        prev_fingerprint
    };

    let tags_json =
        serde_json::to_string(&new_tags).map_err(|e| format!("serialize tags: {}", e))?;
    let now = chrono::Utc::now().to_rfc3339();

    let (body_inline, body_blob_path): (Option<String>, Option<String>) =
        if new_body.len() > INLINE_BODY_LIMIT {
            let path = write_blob(blobs_dir, &new_body)?;
            (None, Some(path))
        } else {
            (Some(new_body.clone()), None)
        };

    let recur_bump = patch.bump_recurrence.unwrap_or(0);
    let pinned_param: Option<i64> = patch.pinned.map(|b| if b { 1 } else { 0 });
    conn.execute(
        r#"UPDATE mem_items
           SET title = ?1, summary = ?2, area = ?3, tags_json = ?4,
               body_inline = ?5, body_blob_path = ?6, importance = ?7,
               updated_at = ?8, recurrence_count = recurrence_count + ?10,
               is_pinned_wakeup = COALESCE(?11, is_pinned_wakeup),
               seed_fingerprint = ?12
           WHERE id = ?9"#,
        params![
            &new_title,
            &new_summary,
            &new_area,
            &tags_json,
            &body_inline,
            &body_blob_path,
            new_importance,
            &now,
            &id,
            recur_bump,
            pinned_param,
            &new_fingerprint,
        ],
    )
    .map_err(|e| format!("update mem_item: {}", e))?;

    let tags_text = new_tags.join(" ");
    let _ = conn.execute("DELETE FROM mem_fts WHERE mem_id = ?1", params![&id]);
    conn.execute(
        r#"INSERT INTO mem_fts (mem_id, title, summary, body, tags)
           VALUES (?1, ?2, ?3, ?4, ?5)"#,
        params![
            &id,
            &new_title,
            new_summary.as_deref().unwrap_or(""),
            &new_body,
            &tags_text,
        ],
    )
    .map_err(|e| format!("update fts: {}", e))?;

    // can see the write happened — but leave old_/new_ at None for fields that were unchanged.
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
    if patch.pinned.is_some() && Some(prev_pinned) != patch.pinned {
        dirty = true;
    }
    // Kind is not patchable via MemUpdate, but record it so downstream consumers can see which kind the item was at update time.
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
