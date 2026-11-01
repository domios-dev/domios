use crate::executor::Transaction;
use crate::{db_error, vectors, PostgresMemStore, StoreResult};
use codeterm_mem::{
    backend::{SaveError, WriteContext},
    ids,
    manager_crud::SaveOutcome,
    store::{self, HistoryEntry, HistorySnapshot, MemAuthor, MemAuthorKind, MemUpdate},
    MemItem, NewMemItem,
};
use postgres::Row;
use sha2::{Digest, Sha256};

pub(crate) fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn author(row: &Row) -> StoreResult<MemAuthor> {
    let kind: String = row.try_get("author_kind").map_err(db_error)?;
    Ok(MemAuthor {
        kind: match kind.as_str() {
            "owner" => MemAuthorKind::Owner,
            "neighbor" => MemAuthorKind::Neighbor,
            "system" => MemAuthorKind::System,
            _ => return Err("invalid memory author kind".into()),
        },
        account_uid: row.try_get("author_uid").map_err(db_error)?,
        friendship_id: row.try_get("author_friendship").map_err(db_error)?,
        label: row.try_get("author_label").map_err(db_error)?,
    })
}

pub(crate) fn get_in(tx: &mut Transaction<'_>, id: &str) -> StoreResult<Option<MemItem>> {
    let Some(row) = tx.query_opt("SELECT m.*,b.body AS blob FROM mem_items m LEFT JOIN mem_blobs b ON b.name=m.body_blob_path WHERE m.id=$1", &[&id]).map_err(db_error)? else { return Ok(None); };
    let inline: Option<String> = row.get("body_inline");
    let mut body = match inline {
        Some(body) => body,
        None => {
            let blob: Option<Vec<u8>> = row.get("blob");
            match blob {
                Some(bytes) => {
                    String::from_utf8(bytes).map_err(|_| "invalid memory blob encoding")?
                }
                None if row.get::<_, Option<String>>("body_blob_path").is_some() => {
                    return Err("memory blob body missing".into())
                }
                None => String::new(),
            }
        }
    };
    let mut updated_at: String = row.get("updated_at");
    if let Some(path) = row
        .get::<_, Option<String>>("source")
        .as_deref()
        .and_then(|s| s.strip_prefix("file:"))
    {
        let recently_accessed = row
            .get::<_, Option<String>>("accessed_at")
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .is_some_and(|t| {
                (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds() < 10
            });
        if !recently_accessed {
            if let Ok(meta) = std::fs::metadata(path) {
                let size = meta.len() as i64;
                let mtime = meta
                    .modified()
                    .ok()
                    .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
                if row.get::<_, Option<i64>>("source_size") != Some(size)
                    || row.get::<_, Option<String>>("source_mtime") != mtime
                {
                    let read_only: String = tx
                        .query_one("SHOW transaction_read_only", &[])
                        .map_err(db_error)?
                        .get(0);
                    if read_only == "off" {
                        let bumped = now();
                        tx.execute("UPDATE mem_items SET source_size=$1,source_mtime=$2,updated_at=$3 WHERE id=$4", &[&size,&mtime,&bumped,&id]).map_err(db_error)?;
                        updated_at = bumped;
                    }
                }
            }
        }
        if let Ok(content) = std::fs::read_to_string(path) {
            if !content.trim().is_empty() {
                body = content.trim().to_owned();
            }
        }
    }
    Ok(Some(MemItem {
        author: author(&row)?,
        id: row.get("id"),
        uid: row.get("uid"),
        kind: row.get("kind"),
        scope: row.get("scope"),
        title: row.get("title"),
        summary: row.get("summary"),
        area: row.get("area"),
        tags: serde_json::from_str(&row.get::<_, String>("tags_json"))
            .map_err(|_| "invalid memory tags")?,
        body,
        source: row.get("source"),
        importance: row.get("importance"),
        created_at: row.get("created_at"),
        updated_at,
        accessed_at: row.get("accessed_at"),
        access_count: row.get("access_count"),
        recurrence_count: row.get("recurrence_count"),
    }))
}

pub(crate) fn body_in(
    tx: &mut Transaction<'_>,
    body: &str,
) -> StoreResult<(Option<String>, Option<String>)> {
    if body.len() <= 2048 {
        return Ok((Some(body.into()), None));
    }
    let name = format!("sha256_{:x}.txt", Sha256::digest(body.as_bytes()));
    tx.execute(
        "INSERT INTO mem_blobs(name,body) VALUES ($1,$2) ON CONFLICT DO NOTHING",
        &[&name, &body.as_bytes()],
    )
    .map_err(db_error)?;
    Ok((None, Some(name)))
}

pub(crate) fn history_in(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    id: &str,
    change: &str,
    old: &HistorySnapshot,
    new: &HistorySnapshot,
    reason: Option<&str>,
) -> StoreResult<()> {
    tx.execute("INSERT INTO mem_item_history(item_id,changed_at,changed_by,change_type,old_title,new_title,old_body,new_body,old_importance,new_importance,old_kind,new_kind,reason,author_kind,author_uid,author_friendship,author_label) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)",
        &[&id,&now(),&ctx.changed_by,&change,&old.title,&new.title,&old.body,&new.body,&old.importance,&new.importance,&old.kind,&new.kind,&reason,&ctx.author.kind.as_str(),&ctx.author.account_uid,&ctx.author.friendship_id,&ctx.author.label]).map_err(db_error)?;
    Ok(())
}

pub(crate) fn snapshot(item: &MemItem) -> HistorySnapshot {
    HistorySnapshot {
        title: Some(item.title.clone()),
        body: Some(item.body.clone()),
        importance: Some(item.importance),
        kind: Some(item.kind.clone()),
    }
}

fn sources(item: &NewMemItem) -> (Option<String>, Option<String>) {
    if item.kind == codeterm_mem::MemKind::Skill {
        (
            item.source
                .as_deref()
                .map(codeterm_mem_store::skill_folders::normalize_file_source),
            item.source_path
                .as_deref()
                .map(codeterm_mem_store::skill_folders::normalize_file_source),
        )
    } else {
        (item.source.clone(), item.source_path.clone())
    }
}

pub(crate) fn fts_in(tx: &mut Transaction<'_>, id: &str, item: &NewMemItem) -> StoreResult<()> {
    tx.execute("DELETE FROM mem_fts WHERE mem_id=$1", &[&id])
        .map_err(db_error)?;
    tx.execute(
        "INSERT INTO mem_fts(mem_id,title,summary,body,tags) VALUES ($1,$2,$3,$4,$5)",
        &[
            &id,
            &item.title,
            &item.summary.as_deref().unwrap_or(""),
            &item.body,
            &item.tags.join(" "),
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

pub(crate) fn insert_in(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    item: &NewMemItem,
    fingerprint: Option<&str>,
) -> StoreResult<String> {
    store::guard_instruction(&ctx.author, item.kind.as_str(), item.pinned)?;
    let slug = ids::slugify(&item.title);
    let candidate = ids::compose(item.kind.as_str(), &slug);
    let id = if get_in(tx, &candidate)?.is_some() {
        ids::with_collision_suffix(item.kind.as_str(), &slug, &item.body)
    } else {
        candidate
    };
    let (inline, blob) = body_in(tx, &item.body)?;
    let (source, source_path) = sources(item);
    let (size, mtime) = source_path
        .as_deref()
        .and_then(|p| std::fs::metadata(p.strip_prefix("file:").unwrap_or(p)).ok())
        .map(|metadata| {
            (
                Some(metadata.len() as i64),
                metadata
                    .modified()
                    .ok()
                    .map(|time| chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339()),
            )
        })
        .unwrap_or_default();
    let timestamp = now();
    let tags = serde_json::to_string(&item.tags).map_err(|_| "serialize memory tags")?;
    tx.execute("INSERT INTO mem_items(id,uid,kind,scope,title,summary,area,tags_json,body_inline,body_blob_path,source,importance,created_at,updated_at,source_path,source_size,source_mtime,is_pinned_wakeup,seed_fingerprint,author_kind,author_uid,author_friendship,author_label) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22)",
        &[&id,&ulid::Ulid::new().to_string(),&item.kind.as_str(),&item.scope.as_str(),&item.title,&item.summary,&item.area,&tags,&inline,&blob,&source,&item.importance,&timestamp,&source_path,&size,&mtime,&i64::from(item.pinned),&fingerprint,&ctx.author.kind.as_str(),&ctx.author.account_uid,&ctx.author.friendship_id,&ctx.author.label]).map_err(db_error)?;
    if let Some(path) = source_path {
        tx.execute(
            "DELETE FROM mem_source_tombstones WHERE source_key=$1",
            &[&path],
        )
        .map_err(db_error)?;
    }
    fts_in(tx, &id, item)?;
    history_in(
        tx,
        ctx,
        &id,
        "create",
        &HistorySnapshot::default(),
        &HistorySnapshot {
            title: Some(item.title.clone()),
            body: Some(item.body.clone()),
            importance: Some(item.importance),
            kind: Some(item.kind.as_str().into()),
        },
        None,
    )?;
    Ok(id)
}

pub(crate) fn replace_in(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    id: &str,
    item: &NewMemItem,
) -> StoreResult<()> {
    let previous = get_in(tx, id)?.ok_or("mem item not found")?;
    store::guard_instruction(&ctx.author, item.kind.as_str(), item.pinned)?;
    store::guard_instruction(&previous.author, item.kind.as_str(), item.pinned)?;
    let extra = tx
        .query_one(
            "SELECT source_path,is_pinned_wakeup FROM mem_items WHERE id=$1",
            &[&id],
        )
        .map_err(db_error)?;
    let (source, path) = sources(item);
    let mut changed = Vec::new();
    for (different, field) in [
        (previous.author != ctx.author, "author"),
        (previous.title != item.title, "title"),
        (previous.body != item.body, "body"),
        (previous.summary != item.summary, "summary"),
        (previous.area != item.area, "area"),
        (previous.tags != item.tags, "tags"),
        (previous.scope != item.scope.as_str(), "scope"),
        (previous.source != source, "source"),
        (extra.get::<_, Option<String>>(0) != path, "source_path"),
        (extra.get::<_, i64>(1) != i64::from(item.pinned), "pinned"),
        (
            (previous.importance - item.importance).abs() > f64::EPSILON,
            "importance",
        ),
    ] {
        if different {
            changed.push(field);
        }
    }
    if changed.is_empty() {
        return Ok(());
    }
    write_payload_in(tx, ctx, id, item)?;
    let current = get_in(tx, id)?.ok_or("mem item disappeared")?;
    history_in(
        tx,
        &ctx.with_author(previous.author.clone()),
        id,
        "update",
        &snapshot(&previous),
        &snapshot(&current),
        Some(&format!("fields={}", changed.join(","))),
    )
}

fn write_payload_in(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    id: &str,
    item: &NewMemItem,
) -> StoreResult<()> {
    let (source, path) = sources(item);
    let (inline, blob) = body_in(tx, &item.body)?;
    tx.execute("UPDATE mem_items SET title=$2,summary=$3,area=$4,tags_json=$5,body_inline=$6,body_blob_path=$7,source=$8,importance=$9,updated_at=$10,source_path=$11,is_pinned_wakeup=$12,scope=$13,seed_fingerprint=NULL,author_kind=$14,author_uid=$15,author_friendship=$16,author_label=$17 WHERE id=$1",
        &[&id,&item.title,&item.summary,&item.area,&serde_json::to_string(&item.tags).map_err(|_| "serialize memory tags")?,&inline,&blob,&source,&item.importance,&now(),&path,&i64::from(item.pinned),&item.scope.as_str(),&ctx.author.kind.as_str(),&ctx.author.account_uid,&ctx.author.friendship_id,&ctx.author.label]).map_err(db_error)?;
    fts_in(tx, id, item)?;
    Ok(())
}

pub(crate) fn payload_in(
    tx: &mut Transaction<'_>,
    id: &str,
) -> StoreResult<Option<(NewMemItem, Option<String>)>> {
    let Some(item) = get_in(tx, id)? else {
        return Ok(None);
    };
    let row = tx
        .query_one(
            "SELECT source_path,seed_fingerprint,is_pinned_wakeup FROM mem_items WHERE id=$1",
            &[&id],
        )
        .map_err(db_error)?;
    Ok(Some((
        NewMemItem {
            kind: codeterm_mem::MemKind::from_str(&item.kind).ok_or("invalid managed item kind")?,
            scope: store::MemScope::from_str(&item.scope)?,
            title: item.title,
            summary: item.summary,
            area: item.area,
            tags: item.tags,
            body: item.body,
            source: item.source,
            importance: item.importance,
            source_path: row.get(0),
            pinned: row.get::<_, i64>(2) != 0,
        },
        row.get(1),
    )))
}

pub(crate) fn update_in(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    id: &str,
    patch: &MemUpdate,
) -> StoreResult<bool> {
    if patch.area.is_some()
        && patch.title.is_none()
        && patch.summary.is_none()
        && patch.tags.is_none()
        && patch.body.is_none()
        && patch.importance.is_none()
        && patch.bump_recurrence.is_none()
        && patch.pinned.is_none()
    {
        if let Some(area) = &patch.area {
            return crate::curation::area_in(tx, ctx, id, area, None);
        }
    }
    let Some(previous) = get_in(tx, id)? else {
        return Ok(false);
    };
    let extra = tx
        .query_one(
            "SELECT source_path,is_pinned_wakeup,seed_fingerprint FROM mem_items WHERE id=$1",
            &[&id],
        )
        .map_err(db_error)?;
    let old_pinned = extra.get::<_, i64>(1) != 0;
    let item = NewMemItem {
        kind: codeterm_mem::MemKind::from_str(&previous.kind)
            .unwrap_or(codeterm_mem::MemKind::Note),
        scope: store::MemScope::from_str(&previous.scope)?,
        title: patch
            .title
            .clone()
            .unwrap_or_else(|| previous.title.clone()),
        summary: patch.summary.clone().or_else(|| previous.summary.clone()),
        area: patch.area.clone().or_else(|| previous.area.clone()),
        tags: patch.tags.clone().unwrap_or_else(|| previous.tags.clone()),
        body: patch.body.clone().unwrap_or_else(|| previous.body.clone()),
        source: previous.source.clone(),
        importance: patch.importance.unwrap_or(previous.importance),
        source_path: extra.get(0),
        pinned: patch.pinned.unwrap_or(old_pinned),
    };
    let writes_text = patch.title.is_some()
        || patch.summary.is_some()
        || patch.tags.is_some()
        || patch.body.is_some();
    let next_author = if previous.author.is_neighbor() || !writes_text {
        previous.author.clone()
    } else {
        ctx.author.clone()
    };
    store::guard_instruction(&ctx.author, &previous.kind, item.pinned)?;
    store::guard_instruction(&next_author, &previous.kind, item.pinned)?;
    let mut changed = Vec::new();
    let mut old = HistorySnapshot::default();
    let mut new = HistorySnapshot::default();
    if previous.title != item.title {
        old.title = Some(previous.title.clone());
        new.title = Some(item.title.clone());
        changed.push("title");
    }
    if previous.body != item.body {
        old.body = Some(previous.body.clone());
        new.body = Some(item.body.clone());
        changed.push("body");
    }
    if (previous.importance - item.importance).abs() > f64::EPSILON {
        old.importance = Some(previous.importance);
        new.importance = Some(item.importance);
        changed.push("importance");
    }
    if previous.summary != item.summary {
        changed.push("summary");
    }
    if previous.area != item.area {
        changed.push("area");
    }
    if previous.tags != item.tags {
        changed.push("tags");
    }
    if old_pinned != item.pinned {
        changed.push("pinned");
    }
    let bump = patch.bump_recurrence.unwrap_or(0);
    if changed.is_empty() && bump == 0 {
        return Ok(false);
    }
    write_payload_in(tx, &ctx.with_author(next_author), id, &item)?;
    let fingerprint: Option<String> = if changed.is_empty() {
        extra.get(2)
    } else {
        None
    };
    tx.execute(
        "UPDATE mem_items SET recurrence_count=recurrence_count+$2,seed_fingerprint=$3 WHERE id=$1",
        &[&id, &bump, &fingerprint],
    )
    .map_err(db_error)?;
    if !changed.is_empty() {
        old.kind = Some(previous.kind.clone());
        new.kind = Some(previous.kind);
        history_in(
            tx,
            ctx,
            id,
            "update",
            &old,
            &new,
            Some(&format!("fields={}", changed.join(","))),
        )?;
    }
    Ok(true)
}

pub(crate) fn save_in(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    item: &NewMemItem,
    replace: bool,
) -> Result<SaveOutcome, SaveError> {
    let existing = if item.title.is_empty() {
        None
    } else {
        tx.query_opt(
            "SELECT id FROM mem_items WHERE kind=$1 AND title=$2 ORDER BY id LIMIT 1 FOR UPDATE",
            &[&item.kind.as_str(), &item.title],
        )
        .map_err(db_error)?
        .map(|row| row.get::<_, String>(0))
    };
    let candidate = ids::compose(item.kind.as_str(), &ids::slugify(&item.title));
    let existing = match existing {
        Some(id) => Some(id),
        None if replace && get_in(tx, &candidate)?.is_some() => Some(candidate),
        None => None,
    };
    if let Some(id) = existing {
        let previous = get_in(tx, &id)?.ok_or_else(|| "mem item disappeared".to_owned())?;
        if let Some(guard) = &ctx.replace_guard {
            guard.check_insert(item.area.as_deref().unwrap_or(""))?;
            let pinned: i64 = tx
                .query_one("SELECT is_pinned_wakeup FROM mem_items WHERE id=$1", &[&id])
                .map_err(db_error)?
                .get(0);
            let kind = codeterm_mem::MemKind::from_str(&previous.kind)
                .ok_or(SaveError::Guard(store::ReplaceGuardError::TitleConflict))?;
            guard.check_replace(previous.area.as_deref().unwrap_or(""), kind, pinned != 0)?;
        }
        let normalize = |body: &str| body.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalize(&previous.body) == normalize(&item.body) && previous.author == ctx.author {
            tx.execute("UPDATE mem_items SET recurrence_count=recurrence_count+1,accessed_at=$2 WHERE id=$1", &[&id,&now()]).map_err(db_error)?;
            return Ok(SaveOutcome {
                id,
                outcome: "unchanged".into(),
            });
        }
        replace_in(tx, ctx, &id, item)?;
        return Ok(SaveOutcome {
            id,
            outcome: "updated".into(),
        });
    }
    if let Some(guard) = &ctx.replace_guard {
        guard.check_insert(item.area.as_deref().unwrap_or(""))?;
    }
    Ok(SaveOutcome {
        id: insert_in(tx, ctx, item, None)?,
        outcome: "created".into(),
    })
}

pub(crate) fn delete_in(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    id: &str,
) -> StoreResult<bool> {
    let Some(item) = get_in(tx, id)? else {
        return Ok(false);
    };
    history_in(
        tx,
        ctx,
        id,
        "delete",
        &snapshot(&item),
        &HistorySnapshot::default(),
        None,
    )?;
    tx.execute("DELETE FROM mem_fts WHERE mem_id=$1", &[&id])
        .map_err(db_error)?;
    vectors::clear_in(tx, id)?;
    tx.execute("DELETE FROM relations WHERE from_id=$1 OR to_id=$1", &[&id])
        .map_err(db_error)?;
    tx.execute(
        "DELETE FROM entity_aliases WHERE alias_id=$1 OR canonical_id=$1",
        &[&id],
    )
    .map_err(db_error)?;
    tx.execute("DELETE FROM mem_items WHERE id=$1", &[&id])
        .map_err(db_error)?;
    Ok(true)
}

pub(crate) fn histories_in(tx: &mut Transaction<'_>, id: &str) -> StoreResult<Vec<HistoryEntry>> {
    tx.query(
        "SELECT * FROM mem_item_history WHERE item_id=$1 ORDER BY changed_at,id",
        &[&id],
    )
    .map_err(db_error)?
    .into_iter()
    .map(|row| {
        let reason: Option<String> = row.get("reason");
        let changed_fields = reason
            .as_deref()
            .and_then(|r| r.strip_prefix("fields="))
            .map(|fields| fields.split(',').map(str::to_owned).collect())
            .or_else(|| {
                reason
                    .as_deref()
                    .filter(|r| r.starts_with("area_merge=") || r.starts_with("area_update="))
                    .map(|_| vec!["area".into()])
            })
            .unwrap_or_default();
        Ok(HistoryEntry {
            author: author(&row)?,
            id: row.get("id"),
            item_id: row.get("item_id"),
            changed_at: row.get("changed_at"),
            changed_by: row.get("changed_by"),
            change_type: row.get("change_type"),
            changed_fields,
            old_title: row.get("old_title"),
            new_title: row.get("new_title"),
            old_body: row.get("old_body"),
            new_body: row.get("new_body"),
            old_importance: row.get("old_importance"),
            new_importance: row.get("new_importance"),
            old_kind: row.get("old_kind"),
            new_kind: row.get("new_kind"),
            reason,
        })
    })
    .collect()
}

impl PostgresMemStore {
    pub fn save(
        &self,
        ctx: &WriteContext,
        item: &NewMemItem,
        replace: bool,
    ) -> Result<SaveOutcome, SaveError> {
        self.transaction_error(|tx| save_in(tx, ctx, item, replace))
    }
    pub fn get(&self, id: &str) -> StoreResult<Option<MemItem>> {
        self.transaction(|tx| {
            let mut item = get_in(tx, id)?;
            if !self.read_only {
                crate::queries::touch_in(tx, &[id.to_owned()])?;
                if let Some(item) = &mut item {
                    let row = tx
                        .query_one(
                            "SELECT accessed_at,access_count FROM mem_items WHERE id=$1",
                            &[&id],
                        )
                        .map_err(db_error)?;
                    item.accessed_at = row.get(0);
                    item.access_count = row.get(1);
                }
            }
            Ok(item)
        })
    }
    pub fn insert(&self, ctx: &WriteContext, item: &NewMemItem) -> StoreResult<String> {
        self.transaction(|tx| insert_in(tx, ctx, item, None))
    }
    pub fn update(&self, ctx: &WriteContext, id: &str, patch: &MemUpdate) -> StoreResult<bool> {
        self.transaction(|tx| update_in(tx, ctx, id, patch))
    }
    pub fn delete(&self, ctx: &WriteContext, id: &str) -> StoreResult<bool> {
        self.transaction(|tx| delete_in(tx, ctx, id))
    }
    pub fn get_item_history(&self, id: &str) -> StoreResult<Vec<HistoryEntry>> {
        self.transaction(|tx| histories_in(tx, id))
    }
}
