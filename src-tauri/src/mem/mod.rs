//! Unified mem store — one SQLite, one CLI, one query path. See docs/plans/mem-architecture.md.

pub mod store;
pub mod search;
pub mod graph;
pub mod layers;
pub mod extract;
pub mod seed;
pub mod ids;
pub mod learnings;
pub mod dreaming;
pub mod rerank;
pub mod embeddings;
pub mod embed_queue;
pub mod worker_pool;
pub mod activity;
pub mod workers;
pub mod auto_extract;
pub mod file_sync;

// TODO(mesh-sync): cross-machine replication — see docs/plans/mem-architecture.md "Mesh replication policy".

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

pub use store::{MemItem, MemKind, MemScope, NewMemItem};
pub use graph::{Entity, Relation};
pub use search::{SearchHit, SearchQuery};
pub use layers::WakeUpOptions;

pub struct MemManager {
    conn: Mutex<Connection>,
    db_path: PathBuf,
    blobs_dir: PathBuf,
    embed_queue: embed_queue::EmbedQueue,
}

impl MemManager {
    pub fn open(data_dir: &std::path::Path) -> Result<Arc<Self>, String> {
        let mem_dir = data_dir.join("mem");
        std::fs::create_dir_all(&mem_dir)
            .map_err(|e| format!("create mem dir: {}", e))?;
        let blobs_dir = mem_dir.join("blobs");
        std::fs::create_dir_all(&blobs_dir)
            .map_err(|e| format!("create blobs dir: {}", e))?;

        // sqlite-vec must register BEFORE Connection::open
        embeddings::ensure_extension_registered();

        let db_path = mem_dir.join("knowledge.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("open knowledge.db: {}", e))?;
        // concurrent writers wait up to 5s instead of failing
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("set busy_timeout: {}", e))?;

        store::init_schema(&conn)
            .map_err(|e| format!("init schema: {}", e))?;

        // non-fatal — FTS5 hybrid still works without vectors
        if let Err(e) = embeddings::init_extension(&conn) {
            log::warn!("[mem] sqlite-vec init failed: {} — semantic recall disabled", e);
        }

        let embed_queue = embed_queue::EmbedQueue::start(db_path.clone());

        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            db_path,
            blobs_dir,
            embed_queue,
        }))
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Arc<Self>, String> {
        let conn = Connection::open_in_memory()
            .map_err(|e| format!("open :memory:: {}", e))?;
        store::init_schema(&conn)
            .map_err(|e| format!("init schema: {}", e))?;
        let tmp = std::env::temp_dir().join(format!("mem-test-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(tmp.join("blobs")).ok();
        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            db_path: tmp.join("knowledge.db"),
            blobs_dir: tmp.join("blobs"),
            embed_queue: embed_queue::EmbedQueue::disabled(),
        }))
    }

    /// Offline / deterministic constructor for benches, evals, and tests
    /// outside the main test harness.
    ///
    /// Differences from [`MemManager::open`]:
    /// - No background embed queue worker thread is spawned. Any body that
    ///   would normally be enqueued for indexing is silently dropped.
    /// - No sqlite-vec extension warning if init fails (bench mode falls
    ///   back to FTS5-only recall by design).
    /// - `data_dir` is the final directory; the caller is responsible for
    ///   picking a tempdir path. `knowledge.db` + `blobs/` are created
    ///   inside it exactly like `open`.
    ///
    /// This constructor exists so the mem-recall bench runner can exercise
    /// the real insert / FTS search path without spinning up tokio tasks,
    /// calling any LLM, or requiring API keys / model weights.
    pub fn open_offline(data_dir: &std::path::Path) -> Result<Arc<Self>, String> {
        std::fs::create_dir_all(data_dir)
            .map_err(|e| format!("create data dir: {}", e))?;
        let blobs_dir = data_dir.join("blobs");
        std::fs::create_dir_all(&blobs_dir)
            .map_err(|e| format!("create blobs dir: {}", e))?;

        embeddings::ensure_extension_registered();

        let db_path = data_dir.join("knowledge.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("open knowledge.db: {}", e))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("set busy_timeout: {}", e))?;

        store::init_schema(&conn)
            .map_err(|e| format!("init schema: {}", e))?;

        // Non-fatal: offline mode is allowed to run FTS5-only without vectors.
        let _ = embeddings::init_extension(&conn);

        Ok(Arc::new(Self {
            conn: Mutex::new(conn),
            db_path,
            blobs_dir,
            embed_queue: embed_queue::EmbedQueue::disabled(),
        }))
    }

    pub fn db_path(&self) -> &std::path::Path { &self.db_path }
    pub fn blobs_dir(&self) -> &std::path::Path { &self.blobs_dir }

}

#[cfg(feature = "embeddings")]
/// Build the embedding input text for an item.
/// Format: "kind: title [area] #tag1 #tag2\n\nbody" — compact header so
/// the embedder captures title/kind/area/tags alongside body content.
pub fn compose_embed_text(
    kind: &str,
    title: &str,
    area: Option<&str>,
    tags: &[String],
    body: &str,
) -> String {
    let mut header = format!("{}: {}", kind, title);
    if let Some(a) = area {
        if !a.is_empty() {
            header.push_str(&format!(" [{}]", a));
        }
    }
    for tag in tags.iter().filter(|t| !t.is_empty()) {
        header.push_str(&format!(" #{}", tag));
    }
    format!("{}\n\n{}", header, body)
}

fn query_missing_vectors(conn: &rusqlite::Connection) -> Vec<(String, String)> {
    let sql = r#"SELECT m.id, m.kind, m.title, m.area, m.tags_json, COALESCE(m.body_inline, '')
                 FROM mem_items m
                 LEFT JOIN mem_vectors v ON v.mem_id = m.id
                 WHERE v.mem_id IS NULL AND m.body_inline IS NOT NULL AND LENGTH(m.body_inline) > 10"#;
    let mut out = Vec::new();
    if let Ok(mut stmt) = conn.prepare(sql) {
        if let Ok(rows) = stmt.query_map([], |r| {
            let id: String = r.get(0)?;
            let kind: String = r.get(1)?;
            let title: String = r.get(2)?;
            let area: Option<String> = r.get(3)?;
            let tags_json: String = r.get(4)?;
            let body: String = r.get(5)?;
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            Ok((id, compose_embed_text(&kind, &title, area.as_deref(), &tags, &body)))
        }) {
            for row in rows.flatten() {
                out.push(row);
            }
        }
    }
    out
}

const REINDEX_MAX_PASSES: usize = 5;
const REINDEX_PASS_DELAY_SECS: u64 = 60;
const REINDEX_MAX_FAILURES_PER_ITEM: usize = 2;

impl MemManager {
    /// Catch-up: direct index() call (bypass embed_queue) — isolates whether
    /// leak is in the embed_queue worker/channel or in index() itself.
    #[cfg(feature = "embeddings")]
    pub fn reindex_missing(self: &Arc<Self>) {
        let db_path = self.db_path.clone();
        let blobs_dir = self.blobs_dir.clone();
        std::thread::spawn(move || {
            embeddings::ensure_extension_registered();
            let conn = match rusqlite::Connection::open(&db_path) {
                Ok(c) => { let _ = c.busy_timeout(std::time::Duration::from_secs(5)); c }
                Err(e) => { log::warn!("[mem] reindex catch-up: conn: {}", e); return; }
            };
            let missing = query_missing_vectors(&conn);
            if missing.is_empty() {
                log::info!("[mem] reindex catch-up: nothing missing");
                return;
            }
            log::info!("[mem] reindex catch-up: DIRECT index for {} item(s)", missing.len());
            for (id, embed_text) in missing {
                // embed_text already has title prefixed (from query_missing_vectors).
                // If body was empty (blob storage), re-read blob and re-prefix title.
                let actual_text = if embed_text.trim_end().ends_with("\n\n") {
                    let row: Option<(String, Option<String>)> = conn
                        .query_row(
                            "SELECT title, body_blob_path FROM mem_items WHERE id = ?1",
                            rusqlite::params![&id], |r| Ok((r.get(0)?, r.get(1)?)),
                        ).ok();
                    let Some((title, Some(rel))) = row else { continue };
                    let body = std::fs::read_to_string(blobs_dir.join(rel)).unwrap_or_default();
                    format!("{}\n\n{}", title, body)
                } else { embed_text };
                match embeddings::index(&conn, &id, &actual_text) {
                    Ok(()) => log::info!("[mem] reindex direct: indexed {}", id),
                    Err(e) => log::warn!("[mem] reindex direct {}: {}", id, e),
                }
            }
        });
        return;
        let db_path = self.db_path.clone();
        let blobs_dir = self.blobs_dir.clone();
        let queue = self.embed_queue.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(30));
            embeddings::ensure_extension_registered();
            let conn = match rusqlite::Connection::open(&db_path) {
                Ok(c) => {
                    let _ = c.busy_timeout(std::time::Duration::from_secs(5));
                    c
                }
                Err(e) => {
                    log::warn!("[mem] reindex catch-up: open conn failed: {}", e);
                    return;
                }
            };
            let missing = query_missing_vectors(&conn);
            if missing.is_empty() {
                log::info!("[mem] reindex catch-up: nothing missing");
                return;
            }
            log::info!("[mem] reindex catch-up: enqueueing {} item(s) after startup quiet period", missing.len());
            for (id, body) in missing {
                let actual_body = if body.is_empty() {
                    let blob_path: Option<String> = conn
                        .query_row(
                            "SELECT body_blob_path FROM mem_items WHERE id = ?1",
                            rusqlite::params![&id],
                            |r| r.get(0),
                        )
                        .ok()
                        .flatten();
                    let Some(rel) = blob_path else { continue };
                    std::fs::read_to_string(blobs_dir.join(rel)).unwrap_or_default()
                } else {
                    body
                };
                queue.enqueue(id, actual_body);
            }
        });
        return;
        let db_path = self.db_path.clone();
        let blobs_dir = self.blobs_dir.clone();
        std::thread::spawn(move || {
            embeddings::ensure_extension_registered();
            let conn = match rusqlite::Connection::open(&db_path) {
                Ok(c) => {
                    let _ = c.busy_timeout(std::time::Duration::from_secs(5));
                    c
                }
                Err(e) => { log::warn!("[mem] reindex: open failed: {}", e); return; }
            };
            let mut failed_ids: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();

            for pass in 0..REINDEX_MAX_PASSES {
                let missing = query_missing_vectors(&conn);
                if missing.is_empty() {
                    log::info!("[mem] reindex: all items indexed (pass {})", pass);
                    break;
                }
                log::info!("[mem] reindex pass {}: {} items missing vectors", pass, missing.len());
                let mut indexed = 0;
                for (id, body) in &missing {
                    if *failed_ids.get(id.as_str()).unwrap_or(&0) >= REINDEX_MAX_FAILURES_PER_ITEM {
                        continue;
                    }
                    let actual_body = if body.is_empty() {
                        let blob_path: Option<String> = conn.query_row(
                            "SELECT body_blob_path FROM mem_items WHERE id = ?1",
                            rusqlite::params![id],
                            |r| r.get(0),
                        ).ok().flatten();
                        if let Some(rel) = blob_path {
                            std::fs::read_to_string(blobs_dir.join(rel)).unwrap_or_default()
                        } else {
                            continue;
                        }
                    } else {
                        body.clone()
                    };
                    match embeddings::index(&conn, id, &actual_body) {
                        Ok(()) => { indexed += 1; }
                        Err(e) => {
                            let count = failed_ids.entry(id.clone()).or_insert(0);
                            *count += 1;
                            log::warn!("[mem] reindex {}: {} (attempt {})", id, e, count);
                        }
                    }
                }
                if indexed > 0 {
                    log::info!("[mem] reindex pass {}: indexed {} items", pass, indexed);
                }
                if pass + 1 < REINDEX_MAX_PASSES {
                    std::thread::sleep(std::time::Duration::from_secs(REINDEX_PASS_DELAY_SECS));
                }
            }
            if !failed_ids.is_empty() {
                log::warn!("[mem] reindex finished with {} permanently failed items: {:?}",
                    failed_ids.len(), failed_ids.keys().collect::<Vec<_>>());
            }
            log::info!("[mem] reindex thread exiting");
        });
    }

    #[cfg(not(feature = "embeddings"))]
    pub fn reindex_missing(self: &Arc<Self>) {}

    pub(crate) fn conn(&self) -> &parking_lot::Mutex<rusqlite::Connection> {
        &self.conn
    }

    pub(crate) fn queue_embed(&self, id: &str, body: &str) {
        self.embed_queue.enqueue(id.to_string(), body.to_string());
    }

    /// Initialize the sqlite-vec extension and mem_vectors table.
    /// No-op when the `embeddings` feature is not compiled in.
    pub fn init_embeddings(&self) -> Result<(), String> {
        let conn = self.conn.lock();
        embeddings::init_extension(&conn)
    }

    /// Hybrid recall: RRF fusion of FTS5 + vector ANN results.
    /// Always runs both backends and merges via Reciprocal Rank Fusion
    /// for maximum recall quality (typo-tolerant + keyword-precise).
    /// Falls back to FTS5-only when embeddings feature is off.
    pub fn recall(&self, q: &SearchQuery) -> Result<Vec<SearchHit>, String> {
        let conn = self.conn.lock();
        search::hybrid_rrf(&conn, q, &self.blobs_dir)
    }

    /// Run a read-only SQL closure against the underlying connection.
    /// Used by sibling modules that need direct query access.
    pub fn with_conn<R>(&self, f: impl FnOnce(&rusqlite::Connection) -> R) -> R {
        let conn = self.conn.lock();
        f(&*conn)
    }

    /// A/B-testable hybrid recall with custom ranking config.
    pub fn recall_with_config(
        &self,
        q: &SearchQuery,
        cfg: &search::SearchConfig,
    ) -> Result<Vec<SearchHit>, String> {
        let conn = self.conn.lock();
        search::hybrid_rrf_with_config(&conn, q, &self.blobs_dir, cfg)
    }

    pub fn vector_recall(&self, query: &str, k: usize) -> Result<Vec<(String, f64)>, String> {
        let conn = self.conn.lock();
        embeddings::recall(&conn, query, k)
    }

    pub fn index_embedding(&self, id: &str, body: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        embeddings::index(&conn, id, body)
    }

    pub fn save(&self, item: NewMemItem) -> Result<String, String> {
        self.save_inner(item, false)
    }

    /// Raw insert that skips dedup, auto-extraction of child items, and
    /// the embed queue. Intended for bench/eval seeding where we want to
    /// control exactly which items end up in the store. Returns the
    /// generated id (same format as [`save`]).
    pub fn insert_raw(&self, item: &NewMemItem) -> Result<String, String> {
        let conn = self.conn.lock();
        store::insert(&conn, item, &self.blobs_dir)
    }

    pub fn save_replace(&self, item: NewMemItem) -> Result<String, String> {
        self.save_inner(item, true)
    }

    fn save_inner(&self, item: NewMemItem, replace: bool) -> Result<String, String> {
        let conn = self.conn.lock();

        if !replace && !item.title.is_empty() {
            let existing: Option<String> = conn
                .query_row(
                    "SELECT id FROM mem_items WHERE kind = ?1 AND title = ?2 LIMIT 1",
                    rusqlite::params![item.kind.as_str(), &item.title],
                    |r| r.get(0),
                )
                .ok();
            if let Some(dup_id) = existing {
                log::info!("[mem] dedup: bumping recurrence on existing {} (same kind+title)", dup_id);
                let _ = conn.execute(
                    "UPDATE mem_items SET recurrence_count = recurrence_count + 1,
                                         accessed_at = ?1
                     WHERE id = ?2",
                    rusqlite::params![chrono::Utc::now().to_rfc3339(), &dup_id],
                );
                return Ok(dup_id);
            }
        }

        let id = if replace {
            store::upsert(&conn, &item, &self.blobs_dir)?
        } else {
            store::insert(&conn, &item, &self.blobs_dir)?
        };

        #[cfg(feature = "embeddings")]
        {
            let body_text = if let Some(ref src) = item.source {
                if let Some(path) = src.strip_prefix("file:") {
                    std::fs::read_to_string(path).unwrap_or_else(|_| item.body.clone())
                } else {
                    item.body.clone()
                }
            } else {
                item.body.clone()
            };
            let embed_text = compose_embed_text(
                &format!("{:?}", item.kind).to_lowercase(),
                &item.title,
                item.area.as_deref(),
                &item.tags,
                &body_text,
            );
            self.embed_queue.enqueue(id.clone(), embed_text);
        }
        if matches!(
            item.kind,
            MemKind::Conversation | MemKind::Note | MemKind::Daily | MemKind::Learning | MemKind::Fact
        ) {
            let extracted = extract::extract_all(&item.body);
            for ext in extracted {
                let child_kind = match ext.kind {
                    extract::ExtractKind::Preference => MemKind::Preference,
                    extract::ExtractKind::Decision => MemKind::Fact,
                    extract::ExtractKind::Milestone => MemKind::Fact,
                    extract::ExtractKind::Problem => MemKind::Learning,
                    extract::ExtractKind::Emotional => MemKind::Note,
                };
                let body = format!("User has mentioned ({}): {}", ext.label, ext.captured);
                let tags: Vec<String> = ext.kind.tag().split(',').map(|s| s.to_string()).collect();
                let child = NewMemItem {
                    kind: child_kind,
                    scope: item.scope.clone(),
                    title: format!("auto: {}", &body.chars().take(50).collect::<String>()),
                    summary: Some(body.clone()),
                    area: item.area.clone(),
                    tags,
                    body,
                    source: Some(format!("extracted_from:{}", id)),
                    importance: 2.0,
                    source_path: None,
                };
                if let Ok(child_id) = store::insert(&conn, &child, &self.blobs_dir) {
                    let _ = graph::insert_relation(
                        &conn,
                        &Relation {
                            from_id: child_id,
                            relation_type: "extracted_from".into(),
                            to_id: id.clone(),
                            scope: item.scope.as_str().into(),
                            valid_from: chrono::Utc::now().to_rfc3339(),
                            valid_to: None,
                            confidence: 0.8,
                            properties: serde_json::json!({}),
                        },
                    );
                }
            }
        }
        Ok(id)
    }

    pub fn get(&self, id: &str) -> Result<Option<MemItem>, String> {
        let conn = self.conn.lock();
        store::get(&conn, id, &self.blobs_dir)
    }

    pub fn delete(&self, id: &str) -> Result<bool, String> {
        let conn = self.conn.lock();
        store::delete(&conn, id)
    }

    pub fn delete_by_area(&self, area: &str) -> Result<usize, String> {
        let conn = self.conn.lock();
        let ids: Vec<String> = {
            let mut out = Vec::new();
            if let Ok(mut stmt) = conn.prepare("SELECT id FROM mem_items WHERE area = ?1") {
                if let Ok(rows) = stmt.query_map(rusqlite::params![area], |r| r.get::<_, String>(0)) {
                    for row in rows.flatten() {
                        out.push(row);
                    }
                }
            }
            out
        };
        let mut count = 0;
        for id in &ids {
            if store::delete(&conn, id)? {
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn update(&self, id: &str, patch: store::MemUpdate) -> Result<bool, String> {
        let body_changed = patch.body.is_some() || patch.title.is_some();
        let updated = {
            let conn = self.conn.lock();
            store::update(&conn, id, &patch, &self.blobs_dir)?
        };
        if updated && body_changed {
            #[cfg(feature = "embeddings")]
            {
                let conn = self.conn.lock();
                if let Ok(Some(item)) = store::get(&conn, id, &self.blobs_dir) {
                    let embed_text = compose_embed_text(
                        &format!("{:?}", item.kind).to_lowercase(),
                        &item.title,
                        item.area.as_deref(),
                        &item.tags,
                        &item.body,
                    );
                    drop(conn);
                    self.embed_queue.enqueue(id.to_string(), embed_text);
                }
            }
        }
        Ok(updated)
    }

    pub fn search(&self, q: &SearchQuery) -> Result<Vec<SearchHit>, String> {
        let conn = self.conn.lock();
        search::run(&conn, q, &self.blobs_dir)
    }

    pub fn search_with_rerank(
        &self,
        q: &SearchQuery,
        rerank_cfg: &rerank::RerankConfig,
    ) -> Result<Vec<SearchHit>, String> {
        let mut overfetch = q.clone();
        overfetch.limit = Some(q.limit.unwrap_or(5).max(20));
        let candidates = {
            let conn = self.conn.lock();
            search::run(&conn, &overfetch, &self.blobs_dir)?
        };
        let reranker = rerank::build(rerank_cfg);
        Ok(reranker.rerank(&q.query, candidates, q.limit.unwrap_or(5)))
    }

    pub fn wake_up(&self, opts: &WakeUpOptions) -> Result<String, String> {
        let conn = self.conn.lock();
        layers::generate(&conn, opts, &self.blobs_dir)
    }

    pub fn graph_create_entity(&self, e: &Entity) -> Result<(), String> {
        let conn = self.conn.lock();
        graph::insert_entity(&conn, e)
    }

    pub fn graph_relate(&self, r: &Relation) -> Result<(), String> {
        let conn = self.conn.lock();
        graph::upsert_relation(&conn, r)
    }

    pub fn graph_related(
        &self,
        id: &str,
        rel: Option<&str>,
        as_of: Option<&str>,
    ) -> Result<Vec<Relation>, String> {
        let conn = self.conn.lock();
        graph::related(&conn, id, rel, as_of)
    }

    pub fn graph_invalidate(
        &self,
        from_id: &str,
        relation_type: &str,
        to_id: &str,
        ended: Option<&str>,
    ) -> Result<bool, String> {
        let conn = self.conn.lock();
        graph::invalidate_relation(&conn, from_id, relation_type, to_id, ended)
    }

    pub fn graph_timeline(&self, entity_id: &str) -> Result<Vec<Relation>, String> {
        let conn = self.conn.lock();
        graph::timeline(&conn, entity_id)
    }

    pub fn graph_traverse(
        &self,
        start: &str,
        max_hops: usize,
        max_nodes: usize,
        as_of: Option<&str>,
    ) -> Result<graph::TraversalResult, String> {
        let conn = self.conn.lock();
        graph::traverse(&conn, start, max_hops, max_nodes, as_of)
    }

    /// Shortest path between two entities (BFS).
    pub fn graph_shortest_path(
        &self,
        from: &str,
        to: &str,
        max_hops: usize,
    ) -> Result<Option<Vec<Relation>>, String> {
        let conn = self.conn.lock();
        graph::shortest_path(&conn, from, to, max_hops)
    }

    pub fn graph_tunnels(&self) -> Result<Vec<serde_json::Value>, String> {
        let conn = self.conn.lock();
        graph::tunnels(&conn)
    }

    pub fn graph_alias(&self, alias_id: &str, canonical_id: &str) -> Result<(), String> {
        let conn = self.conn.lock();
        graph::add_alias(&conn, alias_id, canonical_id)
    }

    pub fn graph_resolve(&self, id: &str) -> Result<String, String> {
        let conn = self.conn.lock();
        graph::resolve_canonical(&conn, id)
    }

    pub fn graph_aliases(&self, canonical_id: &str) -> Result<Vec<String>, String> {
        let conn = self.conn.lock();
        graph::list_aliases(&conn, canonical_id)
    }

    pub fn seed_bundled_if_needed(&self) -> Result<usize, String> {
        seed::seed_bundled(self)
    }

    pub fn promote_learning(&self, id: &str) -> Result<(), String> {
        let item = self.get(id)?.ok_or_else(|| format!("not found: {}", id))?;
        if item.kind != "learning" {
            return Err(format!("can only promote kind=learning, got kind={}", item.kind));
        }
        let old_importance = item.importance;
        let new_importance = old_importance.max(4.0);
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE mem_items SET kind = 'fact', importance = MAX(importance, 4.0), updated_at = ?1 WHERE id = ?2",
            rusqlite::params![chrono::Utc::now().to_rfc3339(), id],
        ).map_err(|e| format!("promote: {}", e))?;
        store::history_log(
            &conn,
            id,
            "promote",
            &store::HistorySnapshot {
                title: None,
                body: None,
                importance: Some(old_importance),
                kind: Some("learning".into()),
            },
            &store::HistorySnapshot {
                title: None,
                body: None,
                importance: Some(new_importance),
                kind: Some("fact".into()),
            },
            None,
        );
        Ok(())
    }

    pub fn graph_full(&self) -> Result<store::GraphDump, String> {
        let conn = self.conn.lock();
        store::graph_full(&conn)
    }

    pub fn set_positions(
        &self,
        positions: &[(String, f64, f64)],
    ) -> Result<usize, String> {
        let mut conn = self.conn.lock();
        store::set_positions(&mut conn, positions)
    }

    pub fn stats(&self) -> Result<MemStats, String> {
        let conn = self.conn.lock();
        store::stats(&conn)
    }

    /// Full per-item audit trail: every create/update/promote/invalidate/
    /// merge/delete event ever logged for this id, ordered chronologically.
    pub fn get_item_history(&self, id: &str) -> Result<Vec<store::HistoryEntry>, String> {
        let conn = self.conn.lock();
        store::get_item_history(&conn, id)
    }

    /// `{area -> {kind -> count}}` grouped by area (NULL -> "(none)") and
    /// kind. When `workspace` is Some, filter to rows whose scope matches
    /// `workspace:<id>`. When None, aggregate across all scopes.
    pub fn get_taxonomy(
        &self,
        workspace: Option<&str>,
    ) -> Result<std::collections::HashMap<String, std::collections::HashMap<String, u64>>, String>
    {
        let conn = self.conn.lock();
        store::get_taxonomy(&conn, workspace)
    }

    /// Recent mem writes (feeds `mem tail`). When `since` is Some, returns
    /// rows with `updated_at > since`. When None, returns the `limit` most
    /// recent rows.
    pub fn tail_recent(
        &self,
        since: Option<&str>,
        limit: usize,
    ) -> Result<Vec<store::TailEntry>, String> {
        let conn = self.conn.lock();
        store::tail_recent(&conn, since, limit)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemStats {
    pub total_items: u64,
    pub by_kind: std::collections::HashMap<String, u64>,
    pub total_relations: u64,
    pub total_entities: u64,
    #[serde(default)]
    pub total_vectors: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_in_memory_works() {
        let mem = MemManager::open_in_memory().unwrap();
        let stats = mem.stats().unwrap();
        assert_eq!(stats.total_items, 0);
    }

    #[test]
    fn save_and_get_roundtrip() {
        let mem = MemManager::open_in_memory().unwrap();
        let id = mem
            .save(NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: "test fact".into(),
                summary: Some("a test".into()),
                area: Some("test".into()),
                tags: vec!["tag1".into()],
                body: "the body of the fact".into(),
                source: None,
                importance: 3.0,
                source_path: None,
            })
            .unwrap();
        assert!(id.starts_with("fact_"));
        let got = mem.get(&id).unwrap().unwrap();
        assert_eq!(got.title, "test fact");
        assert_eq!(got.body, "the body of the fact");
    }

    #[test]
    fn history_tracks_full_lifecycle_with_promote() {
        let mem = MemManager::open_in_memory().unwrap();
        let id = mem
            .save(NewMemItem {
                kind: MemKind::Learning,
                scope: MemScope::Global,
                title: "lifecycle test".into(),
                summary: None,
                area: Some("test".into()),
                tags: vec![],
                body: "first cut".into(),
                source: None,
                importance: 2.0,
                source_path: None,
            })
            .unwrap();

        mem.update(
            &id,
            store::MemUpdate {
                title: None,
                summary: None,
                area: None,
                tags: None,
                body: Some("refined body".into()),
                importance: Some(3.5),
                bump_recurrence: None,
            },
        )
        .unwrap();

        mem.promote_learning(&id).unwrap();

        let history = mem.get_item_history(&id).unwrap();
        let change_types: Vec<&str> = history.iter().map(|h| h.change_type.as_str()).collect();
        assert!(change_types.contains(&"create"));
        assert!(change_types.contains(&"update"));
        assert!(change_types.contains(&"promote"));
        let promote = history.iter().find(|h| h.change_type == "promote").unwrap();
        assert_eq!(promote.old_kind.as_deref(), Some("learning"));
        assert_eq!(promote.new_kind.as_deref(), Some("fact"));
    }

    #[test]
    fn taxonomy_manager_aggregates_by_area_and_kind() {
        let mem = MemManager::open_in_memory().unwrap();
        for (title, kind, area) in [
            ("t1", MemKind::Fact, Some("deploy")),
            ("t2", MemKind::Note, Some("deploy")),
            ("t3", MemKind::Rule, None),
        ] {
            mem.save(NewMemItem {
                kind,
                scope: MemScope::Global,
                title: title.into(),
                summary: None,
                area: area.map(String::from),
                tags: vec![],
                body: "b".into(),
                source: None,
                importance: 3.0,
                source_path: None,
            })
            .unwrap();
        }
        let tax = mem.get_taxonomy(None).unwrap();
        assert!(tax.get("deploy").and_then(|m| m.get("fact")).is_some());
        assert!(tax.get("(none)").and_then(|m| m.get("rule")).is_some());
    }

    #[test]
    fn preference_auto_extracted_from_note() {
        let mem = MemManager::open_in_memory().unwrap();
        mem.save(NewMemItem {
            kind: MemKind::Note,
            scope: MemScope::Global,
            title: "user said".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "I usually prefer PostgreSQL over MySQL for new projects".into(),
            source: None,
            importance: 3.0,
            source_path: None,
        })
        .unwrap();
        let stats = mem.stats().unwrap();
        assert!(stats.total_items >= 2, "expected ≥2 items, got {}", stats.total_items);
        let pref_count = stats.by_kind.get("preference").copied().unwrap_or(0);
        assert!(pref_count >= 1, "expected ≥1 preference, got {}", pref_count);
    }
}
