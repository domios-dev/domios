use crate::{store, MemKind, MemManager, MemScope, NewMemItem};
use codeterm_blueprint_types::{
    blueprint_document::Document,
    blueprint_memory::{self as model, Entry, Evidence, AREA_CAP, MERGE_BUDGET},
};
use rusqlite::{params, Connection};
use std::sync::atomic::{AtomicU64, Ordering};

const SOURCE: &str = "blueprint-library";
static RECALLS: AtomicU64 = AtomicU64::new(0);
static HITS: AtomicU64 = AtomicU64::new(0);
static DREAM_AREA: AtomicU64 = AtomicU64::new(0);

pub(crate) enum Pending {
    Keep {
        area: String,
        definition: serde_json::Value,
        when_to_use: String,
        evidence: Evidence,
    },
    Reuse(Vec<String>),
}

fn entries(conn: &Connection, blobs: &std::path::Path, area: &str) -> Result<Vec<Entry>, String> {
    let mut stmt=conn.prepare("SELECT id FROM mem_items WHERE kind='skill' AND area=?1 AND source=?2 ORDER BY recurrence_count DESC, accessed_at DESC, id LIMIT 51").map_err(|e|e.to_string())?;
    let ids = stmt
        .query_map(params![area, SOURCE], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    ids.into_iter()
        .map(|id| {
            let item = store::get(conn, &id, blobs)?.ok_or("Blueprint solution disappeared")?;
            let document = model::parse_document(&item.body)?;
            let last_used = item
                .accessed_at
                .as_deref()
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .map_or(0, |time| time.timestamp());
            let failures = item
                .tags
                .iter()
                .find_map(|tag| tag.strip_prefix("failures:")?.parse().ok())
                .unwrap_or(0);
            Ok(Entry {
                id,
                document,
                when_to_use: item.title,
                uses: item.access_count.max(0) as u64,
                successes: item.recurrence_count.max(0) as u64,
                failures,
                kept: item.tags.iter().any(|tag| tag == "explicitly-kept"),
                last_used,
            })
        })
        .collect()
}

fn archive(conn: &Connection, id: &str, reason: &str) -> Result<(), String> {
    crate::curation::calibration::archive::archive_item(
        conn,
        id,
        "skill",
        "system:blueprint-library",
        "archive",
        Some(reason),
    )
}

fn write_entry(conn: &Connection, blobs: &std::path::Path, entry: &Entry) -> Result<(), String> {
    if !model::safe_template(&entry.document, &entry.when_to_use) {
        return Err("Unsafe Blueprint solution cannot be persisted".into());
    }
    let mut tags = vec![
        "blueprint-library".into(),
        format!("failures:{}", entry.failures),
    ];
    if let Some(item) = store::get(conn, &entry.id, blobs)? {
        tags.extend(
            item.tags
                .into_iter()
                .filter(|tag| tag.starts_with("evidence:")),
        );
    }
    if entry.kept {
        tags.push("explicitly-kept".into());
    }
    store::update(
        conn,
        &entry.id,
        &store::MemUpdate {
            title: Some(entry.when_to_use.clone()),
            body: Some(entry.document.yaml()?),
            summary: entry.document.note.clone(),
            tags: Some(tags),
            ..Default::default()
        },
        blobs,
    )?;
    conn.execute(
        "UPDATE mem_items SET recurrence_count=?2, access_count=?3, accessed_at=?4 WHERE id=?1",
        params![
            entry.id,
            entry.successes.min(i64::MAX as u64) as i64,
            entry.uses.min(i64::MAX as u64) as i64,
            chrono::DateTime::from_timestamp(entry.last_used, 0).map(|time| time.to_rfc3339())
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn evidence_prefix(origin: &str) -> String {
    let hash = origin.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    format!("evidence:{hash:016x}:")
}

fn prior_evidence(
    conn: &Connection,
    blobs: &std::path::Path,
    id: &str,
    origin: &str,
) -> Result<(u64, u64), String> {
    let prefix = evidence_prefix(origin);
    let item = store::get(conn, id, blobs)?.ok_or("Blueprint solution disappeared")?;
    Ok(item
        .tags
        .iter()
        .find_map(|tag| {
            let (successes, failures) = tag.strip_prefix(&prefix)?.split_once(':')?;
            Some((successes.parse().ok()?, failures.parse().ok()?))
        })
        .unwrap_or_default())
}

fn record_evidence(
    conn: &Connection,
    blobs: &std::path::Path,
    id: &str,
    evidence: &Evidence,
) -> Result<(), String> {
    let Some(origin) = &evidence.origin else {
        return Ok(());
    };
    let prefix = evidence_prefix(origin);
    let (successes, failures) = prior_evidence(conn, blobs, id, origin)?;
    let mut tags = store::get(conn, id, blobs)?
        .ok_or("Blueprint solution disappeared")?
        .tags;
    tags.retain(|tag| !tag.starts_with(&prefix));
    tags.push(format!(
        "{prefix}{}:{}",
        evidence.successes.max(successes),
        evidence.failures.max(failures)
    ));
    while tags
        .iter()
        .filter(|tag| tag.starts_with("evidence:"))
        .count()
        > 32
    {
        if let Some(index) = tags.iter().position(|tag| tag.starts_with("evidence:")) {
            tags.remove(index);
        }
    }
    store::update(
        conn,
        id,
        &store::MemUpdate {
            tags: Some(tags),
            ..Default::default()
        },
        blobs,
    )?;
    Ok(())
}

impl MemManager {
    fn queue_blueprint(self: &std::sync::Arc<Self>, pending: Pending) -> Result<(), String> {
        let mut queue = self.blueprint_queue.lock();
        if queue.is_none() {
            let (sender, receiver) = std::sync::mpsc::sync_channel::<Pending>(16);
            let weak = std::sync::Arc::downgrade(self);
            std::thread::Builder::new()
                .name("blueprint-memory".into())
                .spawn(move || {
                    while let Ok(pending) = receiver.recv() {
                        let Some(mem) = weak.upgrade() else { break };
                        let result = match pending {
                            Pending::Keep {
                                area,
                                definition,
                                when_to_use,
                                evidence,
                            } => Document::from_definition(&definition)
                                .and_then(|document| {
                                    mem.save_blueprint(&area, document, &when_to_use, evidence)
                                })
                                .map(|_| ()),
                            Pending::Reuse(ids) => mem.reuse_blueprints(&ids),
                        };
                        if let Err(error) = result {
                            log::debug!("[blueprint-memory] write refused: {error}");
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            *queue = Some(sender);
        }
        queue
            .as_ref()
            .ok_or("Blueprint memory queue unavailable")?
            .try_send(pending)
            .map_err(|_| "Blueprint memory queue full or closed".into())
    }

    pub fn enqueue_blueprint(
        self: &std::sync::Arc<Self>,
        area: String,
        definition: serde_json::Value,
        when_to_use: String,
        evidence: Evidence,
    ) -> Result<(), String> {
        self.queue_blueprint(Pending::Keep {
            area,
            definition,
            when_to_use,
            evidence,
        })
    }

    pub fn save_blueprint(
        &self,
        area: &str,
        document: Document,
        when_to_use: &str,
        mut evidence: Evidence,
    ) -> Result<Option<String>, String> {
        self.guard_commit()?;
        let mut document = model::abstract_document(document);
        let hash = model::structure(&document)
            .to_string()
            .bytes()
            .fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            });
        document.name = format!("Saved solution {hash:016x}");
        evidence.safe = model::safe_template(&document, when_to_use);
        let decision = model::decide(&evidence);
        log::debug!(
            "[blueprint-memory] qualification={} save={}",
            decision.reason,
            decision.save
        );
        if !decision.save {
            return Err(format!("Blueprint solution refused: {}", decision.reason));
        }
        if when_to_use.trim().is_empty()
            || when_to_use.chars().count() > 200
            || when_to_use.chars().any(char::is_control)
        {
            return Err("When to use must contain 1..200 plain-text characters".into());
        }
        let yaml = document.yaml()?;
        if yaml.len() > codeterm_blueprint_types::blueprint_recipe::MAX_RECIPE_BYTES {
            return Err("Blueprint solution exceeds document limit".into());
        }
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let mut library = entries(&tx, &self.blobs_dir, area)?;
        let now = chrono::Utc::now();
        let id = if let Some(existing) = model::duplicate(&library, &document) {
            let (prior_successes, prior_failures) = evidence
                .origin
                .as_deref()
                .map(|origin| prior_evidence(&tx, &self.blobs_dir, &existing.id, origin))
                .transpose()?
                .unwrap_or_default();
            let candidate = Entry {
                id: "candidate".into(),
                document,
                when_to_use: when_to_use.into(),
                uses: 0,
                successes: evidence.successes.saturating_sub(prior_successes),
                failures: evidence.failures.saturating_sub(prior_failures),
                kept: evidence.kept,
                last_used: now.timestamp(),
            };
            let mut next = model::merge_pair(existing, &candidate)
                .ok_or("Blueprint structure changed")?
                .keeper;
            next.id = existing.id.clone();
            write_entry(&tx, &self.blobs_dir, &next)?;
            let id = next.id.clone();
            library.retain(|entry| entry.id != id);
            library.push(next);
            id
        } else {
            let id = store::insert(
                &tx,
                &NewMemItem {
                    kind: MemKind::Skill,
                    scope: MemScope::Global,
                    title: format!("Blueprint solution {}", ulid::Ulid::new()),
                    summary: document.note.clone(),
                    area: Some(area.into()),
                    tags: vec!["blueprint-library".into()],
                    body: yaml,
                    source: Some(SOURCE.into()),
                    importance: 0.5,
                    source_path: None,
                    pinned: false,
                },
                &self.blobs_dir,
            )?;
            let next = Entry {
                id: id.clone(),
                document,
                when_to_use: when_to_use.into(),
                uses: 0,
                successes: evidence.successes,
                failures: evidence.failures,
                kept: evidence.kept,
                last_used: now.timestamp(),
            };
            write_entry(&tx, &self.blobs_dir, &next)?;
            library.push(next);
            id
        };
        record_evidence(&tx, &self.blobs_dir, &id, &evidence)?;
        for evicted in model::evictions(&library, AREA_CAP, now.timestamp()) {
            archive(&tx, &evicted, "blueprint solution area cap")?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        let added: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mem_items WHERE source=?1 AND date(created_at)=date('now')",
                [SOURCE],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        drop(conn);
        self.refresh_embedding(&id);
        log::debug!("[blueprint-memory] added_today={added}");
        Ok(Some(id))
    }

    fn reuse_blueprints(&self, ids: &[String]) -> Result<(), String> {
        let conn = self.conn.lock();
        for id in ids.iter().take(5) {
            conn.execute("UPDATE mem_items SET access_count=access_count+1, accessed_at=?2 WHERE id=?1 AND source=?3 AND kind='skill'",params![id,chrono::Utc::now().to_rfc3339(),SOURCE]).map_err(|e|e.to_string())?;
        }
        Ok(())
    }

    pub fn recall_blueprints(
        self: &std::sync::Arc<Self>,
        area: &str,
        intent: &str,
        limit: usize,
    ) -> Result<Vec<Entry>, String> {
        let library = {
            let conn = self.conn.lock();
            entries(&conn, &self.blobs_dir, area)?
        };
        let result = model::recall(&library, intent, limit, chrono::Utc::now().timestamp());
        let total = RECALLS.fetch_add(1, Ordering::Relaxed) + 1;
        if !result.is_empty() {
            HITS.fetch_add(1, Ordering::Relaxed);
        }
        log::debug!(
            "[blueprint-memory] recall_hits={} recalls={total} hit_rate={:.3}",
            HITS.load(Ordering::Relaxed),
            HITS.load(Ordering::Relaxed) as f64 / total as f64
        );
        if !result.is_empty() && !intent.trim().is_empty() {
            let _ = self.queue_blueprint(Pending::Reuse(
                result.iter().map(|entry| entry.id.clone()).collect(),
            ));
        }
        Ok(result)
    }

    pub fn blueprint_recipe(
        self: &std::sync::Arc<Self>,
        area: &str,
        id: &str,
    ) -> Result<Document, String> {
        let library = {
            let conn = self.conn.lock();
            entries(&conn, &self.blobs_dir, area)?
        };
        let entry = library
            .into_iter()
            .find(|entry| entry.id == id)
            .ok_or("Saved Blueprint recipe not found in this project")?;
        Ok(entry.document)
    }

    pub fn record_blueprint_reuse(self: &std::sync::Arc<Self>, id: &str) {
        let _ = self.queue_blueprint(Pending::Reuse(vec![id.into()]));
    }

    pub fn dream_blueprints(&self) -> Result<usize, String> {
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let areas = {
            let count: u64 = tx
                .query_row(
                    "SELECT COUNT(DISTINCT area) FROM mem_items WHERE kind='skill' AND source=?1",
                    [SOURCE],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            let offset = DREAM_AREA.fetch_add(1, Ordering::Relaxed) % count.max(1);
            let mut stmt=tx.prepare("SELECT area FROM mem_items WHERE kind='skill' AND source=?1 GROUP BY area ORDER BY area LIMIT 1 OFFSET ?2").map_err(|e|e.to_string())?;
            let rows = stmt
                .query_map(params![SOURCE, offset], |row| row.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            rows
        };
        let mut changed = Vec::new();
        for area in areas {
            let library = entries(&tx, &self.blobs_dir, &area)?;
            for entry in &library {
                if entry.successes >= 10 && entry.uses >= 5 {
                    log::debug!(
                        "[blueprint-memory] built-in promotion proposal: {}",
                        entry.id
                    );
                }
            }
            for merge in model::dream_merges(&library, MERGE_BUDGET) {
                write_entry(&tx, &self.blobs_dir, &merge.keeper)?;
                archive(
                    &tx,
                    &merge.superseded,
                    &format!("blueprint solution merged into {}", merge.keeper.id),
                )?;
                changed.push(merge.keeper.id);
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        drop(conn);
        for id in &changed {
            self.refresh_embedding(id);
        }
        Ok(changed.len())
    }
}

#[cfg(test)]
mod tests;
