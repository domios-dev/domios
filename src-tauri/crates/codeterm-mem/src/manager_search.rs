//! Mem search and recall operations.

use super::{embeddings, rerank, search, MemManager, SearchHit, SearchQuery, WakeUpOptions};

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
        self.store.recall(q, cfg, &self.embedder, true)
    }

    pub fn vector_recall(&self, query: &str, k: usize) -> Result<Vec<(String, f64)>, String> {
        self.store.vector_recall(query, k, &self.embedder)
    }

    /// Embed outside the connection lock, then store under it.
    pub fn index_embedding(&self, id: &str, body: &str) -> Result<(), String> {
        let vectors = self.embedder.passages(&embeddings::chunk_text(body));
        self.store
            .store_vectors(&Self::write_context(), id, vectors)
    }

    /// Bulk re-embed mem_items and/or graph entities with the active embedder.
    pub fn reembed_bulk(&self, do_items: bool, do_entities: bool) -> Result<ReembedReport, String> {
        let mut report = ReembedReport::default();
        if do_items {
            for (id, text) in self.store.reembed_item_targets()? {
                match self.index_embedding(&id, &text) {
                    Ok(()) => report.items += 1,
                    Err(error) => report.errors.push(format!("item {id}: {error}")),
                }
            }
        }
        if do_entities {
            for (id, text) in self.store.reembed_entity_targets()? {
                match self.index_embedding(&id, &text) {
                    Ok(()) => report.entities += 1,
                    Err(error) => report.errors.push(format!("entity {id}: {error}")),
                }
            }
        }
        Ok(report)
    }

    pub fn search(&self, q: &SearchQuery) -> Result<Vec<SearchHit>, String> {
        self.store.search(q, true)
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
        self.store.recall(q, cfg, &self.embedder, false)
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
        let candidates = self.store.search(&overfetch, record_access)?;
        let reranker = rerank::build(rerank_cfg);
        Ok(reranker.rerank(&q.query, candidates, q.limit.unwrap_or(5)))
    }

    /// Graph-linked mem items in graph rank order, score 0.0; hydration records no access.
    pub fn graph_candidates_untouched(
        &self,
        q: &SearchQuery,
        m: usize,
    ) -> Result<Vec<SearchHit>, String> {
        self.store.graph_candidates(q, m)
    }

    /// Record one access per distinct id, exactly as one hydrating read would.
    pub fn record_access(&self, ids: &[String]) -> Result<(), String> {
        self.store.record_access(&Self::write_context(), ids)
    }

    /// Read-only access counter; `get` would itself count as an access.
    pub fn access_count(&self, id: &str) -> Result<Option<i64>, String> {
        self.store.access_count(id)
    }

    pub fn wake_up(&self, opts: &WakeUpOptions) -> Result<String, String> {
        self.store.wake_up(opts)
    }
}

#[derive(Default, Debug, Clone)]
pub struct ReembedReport {
    pub items: u64,
    pub entities: u64,
    pub errors: Vec<String>,
}
