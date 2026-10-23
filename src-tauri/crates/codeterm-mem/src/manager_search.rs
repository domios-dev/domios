//! Mem search and recall operations.

use super::{
    embeddings, layers, rerank, search, store, MemManager, SearchHit, SearchQuery, WakeUpOptions,
};

impl MemManager {
    /// Hybrid recall: RRF fusion of FTS5 + vector results; FTS-only while the
    /// embedder is unavailable or the index belongs to another model.
    pub fn recall(&self, q: &SearchQuery) -> Result<Vec<SearchHit>, String> {
        self.recall_with_config(q, &search::SearchConfig::default())
    }

    /// A/B-testable hybrid recall with custom ranking config.
    pub fn recall_with_config(
        &self,
        q: &SearchQuery,
        cfg: &search::SearchConfig,
    ) -> Result<Vec<SearchHit>, String> {
        let (embedding, conn) = self.query_under_conn(&q.query);
        search::hybrid_rrf_with_embedding(&conn, q, &self.blobs_dir, cfg, embedding.as_ref())
    }

    pub fn vector_recall(&self, query: &str, k: usize) -> Result<Vec<(String, f64)>, String> {
        let (vector, conn) = self.query_under_conn(query);
        let vector = vector.ok_or_else(|| "query embedding unavailable".to_string())?;
        embeddings::recall_vector(&conn, &vector, k)
    }

    /// Embed outside the connection lock; a publication swaps the model under
    /// that lock, so one re-embed with the newly published model suffices.
    fn query_under_conn(
        &self,
        text: &str,
    ) -> (
        Option<crate::embedder::StampedVector>,
        parking_lot::MutexGuard<'_, rusqlite::Connection>,
    ) {
        let embedding = self.embedder.query(text);
        let conn = self.conn.lock();
        if embedding
            .as_ref()
            .is_none_or(|vector| vector.identity == self.embedder.identity())
        {
            return (embedding, conn);
        }
        drop(conn);
        let embedding = self.embedder.query(text);
        (embedding, self.conn.lock())
    }

    /// Embed outside the connection lock, then store under it.
    pub fn index_embedding(&self, id: &str, body: &str) -> Result<(), String> {
        let vectors = self.embedder.passages(&embeddings::chunk_text(body));
        embeddings::store_vectors(&self.conn.lock(), id, vectors)
    }

    pub(crate) fn reembed_item_targets(
        &self,
        conn: &rusqlite::Connection,
    ) -> Result<Vec<(String, String)>, String> {
        let _read = UntouchedRead::begin(conn)?;
        let mut statement = conn
            .prepare("SELECT id FROM mem_items WHERE kind != 'archived'")
            .map_err(|error| error.to_string())?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        let mut targets = Vec::new();
        for id in ids {
            let Some(item) = store::get(conn, &id, &self.blobs_dir)? else {
                continue;
            };
            if !super::body_is_embeddable(&item.body) {
                continue;
            }
            let text = super::reindex::compose_item_embed_text(
                &item.kind,
                &item.title,
                item.area.as_deref(),
                &item.tags,
                &item.body,
                item.source.as_deref(),
                item.summary.as_deref(),
            );
            targets.push((item.id, text));
        }
        Ok(targets)
    }

    pub(crate) fn reembed_entity_targets(
        &self,
        conn: &rusqlite::Connection,
    ) -> Result<Vec<(String, String)>, String> {
        let mut stmt = conn
            .prepare("SELECT id, entity_type, properties_json FROM entities")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut targets = Vec::new();
        for row in rows {
            let (id, entity_type, props_json) = row.map_err(|e| e.to_string())?;
            let text = embeddings::embeddable_text_for_entity(&id, &entity_type, &props_json);
            targets.push((id, text));
        }
        Ok(targets)
    }

    /// Bulk re-embed mem_items and/or graph entities with the active embedder.
    pub fn reembed_bulk(&self, do_items: bool, do_entities: bool) -> Result<ReembedReport, String> {
        let mut report = ReembedReport::default();
        if do_items {
            for (id, text) in self.reembed_item_targets(&self.conn.lock())? {
                match self.index_embedding(&id, &text) {
                    Ok(()) => report.items += 1,
                    Err(error) => report.errors.push(format!("item {id}: {error}")),
                }
            }
        }
        if do_entities {
            for (id, text) in self.reembed_entity_targets(&self.conn.lock())? {
                match self.index_embedding(&id, &text) {
                    Ok(()) => report.entities += 1,
                    Err(error) => report.errors.push(format!("entity {id}: {error}")),
                }
            }
        }
        Ok(report)
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
        self.rerank_search(q, rerank_cfg, true)
    }

    /// `recall_with_config` that records no access; the caller records it for the hits it returns.
    pub fn recall_with_config_untouched(
        &self,
        q: &SearchQuery,
        cfg: &search::SearchConfig,
    ) -> Result<Vec<SearchHit>, String> {
        let (embedding, conn) = self.query_under_conn(&q.query);
        let _read = UntouchedRead::begin(&conn)?;
        search::hybrid_rrf_with_embedding(&conn, q, &self.blobs_dir, cfg, embedding.as_ref())
    }

    /// `search_with_rerank` that records no access for its overfetched candidates.
    pub fn search_with_rerank_untouched(
        &self,
        q: &SearchQuery,
        rerank_cfg: &rerank::RerankConfig,
    ) -> Result<Vec<SearchHit>, String> {
        self.rerank_search(q, rerank_cfg, false)
    }

    fn rerank_search(
        &self,
        q: &SearchQuery,
        rerank_cfg: &rerank::RerankConfig,
        record_access: bool,
    ) -> Result<Vec<SearchHit>, String> {
        let mut overfetch = q.clone();
        overfetch.limit = Some(q.limit.unwrap_or(5).max(20));
        let candidates = {
            let conn = self.conn.lock();
            let _read = (!record_access)
                .then(|| UntouchedRead::begin(&conn))
                .transpose()?;
            search::run(&conn, &overfetch, &self.blobs_dir)?
        };
        let reranker = rerank::build(rerank_cfg);
        Ok(reranker.rerank(&q.query, candidates, q.limit.unwrap_or(5)))
    }

    /// Graph-linked mem items in graph rank order, score 0.0; hydration records no access.
    pub fn graph_candidates_untouched(
        &self,
        q: &SearchQuery,
        m: usize,
    ) -> Result<Vec<SearchHit>, String> {
        let conn = self.conn.lock();
        let _read = UntouchedRead::begin(&conn)?;
        let mut hits = Vec::new();
        for id in search::graph_source::graph_candidates(&conn, q, m)? {
            if let Some(item) = store::get(&conn, &id, &self.blobs_dir)? {
                hits.push(SearchHit { item, score: 0.0 });
            }
        }
        Ok(hits)
    }

    /// Record one access per distinct id, exactly as one hydrating read would.
    pub fn record_access(&self, ids: &[String]) -> Result<(), String> {
        let conn = self.conn.lock();
        let now = chrono::Utc::now().to_rfc3339();
        let mut seen = std::collections::HashSet::new();
        for id in ids.iter().filter(|id| seen.insert(id.as_str())) {
            store::touch_access(&conn, id, &now).map_err(|e| format!("record access: {e}"))?;
        }
        Ok(())
    }

    /// Read-only access counter; `get` would itself count as an access.
    pub fn access_count(&self, id: &str) -> Result<Option<i64>, String> {
        use rusqlite::OptionalExtension;
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT access_count FROM mem_items WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("read access_count: {e}"))
    }

    pub fn wake_up(&self, opts: &WakeUpOptions) -> Result<String, String> {
        let conn = self.conn.lock();
        layers::generate(&conn, opts, &self.blobs_dir)
    }
}

/// Hydration touches every read item; this savepoint is always rolled back, even on unwind.
/// It also discards hydration's opportunistic file-source refresh, which the next read redoes.
pub(crate) struct UntouchedRead<'c>(&'c rusqlite::Connection);

impl<'c> UntouchedRead<'c> {
    const BEGIN: &'static str = "SAVEPOINT mem_untouched_read";

    pub(crate) fn begin(conn: &'c rusqlite::Connection) -> Result<Self, String> {
        conn.execute_batch(Self::BEGIN)
            .map_err(|e| format!("begin untouched read: {e}"))?;
        Ok(Self(conn))
    }
}

impl Drop for UntouchedRead<'_> {
    fn drop(&mut self) {
        for statement in [
            "ROLLBACK TO mem_untouched_read",
            "RELEASE mem_untouched_read",
        ] {
            if let Err(error) = self.0.execute_batch(statement) {
                log::warn!("[mem.search] untouched read cleanup failed: {error}");
            }
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct ReembedReport {
    pub items: u64,
    pub entities: u64,
    pub errors: Vec<String>,
}
