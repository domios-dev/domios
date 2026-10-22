use crate::solution_metadata::{self as metadata, SolutionMetadata};
use crate::{store, MemKind, MemManager, MemScope, NewMemItem};
use codeterm_blueprint_types::{
    blueprint_document::Document,
    blueprint_memory::{self as model, Entry, Evidence, AREA_CAP, MERGE_BUDGET},
};
use rusqlite::{params, Connection};
use std::sync::atomic::{AtomicU64, Ordering};
mod qualification;
use qualification::PreparedBlueprint;

const SOURCE: &str = "blueprint-library";
static RECALLS: AtomicU64 = AtomicU64::new(0);
static HITS: AtomicU64 = AtomicU64::new(0);
static DREAM_AREA: AtomicU64 = AtomicU64::new(0);

pub(crate) enum Pending {
    Keep {
        area: String,
        prepared: PreparedBlueprint,
    },
    Reuse(Vec<String>),
}

fn entries(conn: &Connection, blobs: &std::path::Path, area: &str) -> Result<Vec<Entry>, String> {
    let mut stmt=conn.prepare("SELECT m.id FROM mem_items m JOIN solution_metadata s ON s.item_id=m.id WHERE m.kind='skill' AND m.area=?1 AND m.source=?2 ORDER BY json_extract(s.metadata_json,'$.blueprint.successes') DESC, json_extract(s.metadata_json,'$.blueprint.last_used') DESC, m.id LIMIT 51").map_err(|error|error.to_string())?;
    let ids = stmt
        .query_map(params![area, SOURCE], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    ids.into_iter()
        .map(|id| {
            let item = store::get(conn, &id, blobs)?.ok_or("Blueprint solution disappeared")?;
            let stats = metadata::read(conn, &id)?
                .and_then(|record| record.blueprint)
                .ok_or("Blueprint metadata absent")?;
            Ok(Entry {
                id,
                document: model::parse_document(&item.body)?,
                when_to_use: stats.when_to_use,
                uses: stats.uses,
                successes: stats.successes,
                failures: stats.failures,
                kept: stats.explicitly_kept,
                last_used: stats.last_used,
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

fn write_entry(
    conn: &Connection,
    blobs: &std::path::Path,
    entry: &Entry,
    mut record: SolutionMetadata,
) -> Result<(), String> {
    record.set_entry(entry);
    let document = &entry.document;
    let mut tags = vec!["blueprint-library".into()];
    if let Some(curator) = &record.curator {
        tags.push("curator".into());
        tags.extend(curator.rounds.keys().cloned());
    }
    store::update(
        conn,
        &entry.id,
        &store::MemUpdate {
            title: Some(document.name.clone()),
            body: Some(document.yaml()?),
            summary: Some(entry.when_to_use.clone()),
            tags: Some(tags),
            ..Default::default()
        },
        blobs,
    )?;
    metadata::write(conn, &entry.id, &record)
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
                            Pending::Keep { area, prepared } => {
                                mem.persist_blueprint(&area, prepared).map(|_| ())
                            }
                            Pending::Reuse(ids) => mem.reuse_blueprints(&ids),
                        };
                        if let Err(error) = result {
                            log::error!("[blueprint-memory] persistence failed: {error}");
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
        self.guard_commit()?;
        let prepared = self.qualify_blueprint(
            &area,
            Document::from_definition(&definition)?,
            &when_to_use,
            evidence,
        )?;
        self.queue_blueprint(Pending::Keep { area, prepared })
    }

    pub fn save_blueprint(
        &self,
        area: &str,
        document: Document,
        when_to_use: &str,
        evidence: Evidence,
    ) -> Result<Option<String>, String> {
        let prepared = self.qualify_blueprint(area, document, when_to_use, evidence)?;
        self.persist_blueprint(area, prepared)
    }

    fn persist_blueprint(
        &self,
        area: &str,
        prepared: PreparedBlueprint,
    ) -> Result<Option<String>, String> {
        self.guard_commit()?;
        let PreparedBlueprint {
            document,
            when_to_use,
            evidence,
        } = prepared;
        let yaml = document.yaml()?;
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let mut library = entries(&tx, &self.blobs_dir, area)?;
        let now = chrono::Utc::now();
        let id = if let Some(existing) = model::duplicate(&library, &document) {
            let mut record =
                metadata::read(&tx, &existing.id)?.ok_or("Blueprint metadata absent")?;
            let prior = record.prior(evidence.origin.as_deref());
            let candidate = Entry {
                id: "candidate".into(),
                document,
                when_to_use: when_to_use.clone(),
                uses: 0,
                successes: evidence.successes.saturating_sub(prior.successes),
                failures: evidence.failures.saturating_sub(prior.failures),
                kept: evidence.kept,
                last_used: now.timestamp(),
            };
            let mut next = model::merge_pair(existing, &candidate)
                .ok_or("Blueprint structure changed")?
                .keeper;
            next.id = existing.id.clone();
            record.observe(&evidence, now.timestamp());
            write_entry(&tx, &self.blobs_dir, &next, record)?;
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
                when_to_use: when_to_use.clone(),
                uses: 0,
                successes: evidence.successes,
                failures: evidence.failures,
                kept: evidence.kept,
                last_used: now.timestamp(),
            };
            let mut record = SolutionMetadata::default();
            record.observe(&evidence, now.timestamp());
            write_entry(&tx, &self.blobs_dir, &next, record)?;
            library.push(next);
            id
        };
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
        let tx = conn
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        for id in ids.iter().take(5) {
            let Some(mut record) = metadata::read(&tx, id)? else {
                continue;
            };
            let stats = record
                .blueprint
                .as_mut()
                .ok_or("Blueprint metadata absent")?;
            stats.uses = stats.uses.saturating_add(1);
            stats.last_used = chrono::Utc::now().timestamp();
            metadata::write(&tx, id, &record)?;
        }
        tx.commit().map_err(|error| error.to_string())
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

    pub fn list_blueprint_solutions(&self, area: &str) -> Result<Vec<Entry>, String> {
        entries(&self.conn.lock(), &self.blobs_dir, area)
    }

    pub fn archive_blueprint_solution(&self, area: &str, id: &str) -> Result<(), String> {
        self.guard_commit()?;
        let conn = self.conn.lock();
        if !entries(&conn, &self.blobs_dir, area)?
            .iter()
            .any(|entry| entry.id == id)
        {
            return Err("Saved Blueprint recipe not found in this project".into());
        }
        archive(&conn, id, "Archived by owner")
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
                let mut record =
                    metadata::read(&tx, &merge.keeper.id)?.ok_or("Blueprint metadata absent")?;
                record.merge(
                    metadata::read(&tx, &merge.superseded)?.ok_or("Blueprint metadata absent")?,
                );
                write_entry(&tx, &self.blobs_dir, &merge.keeper, record)?;
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
