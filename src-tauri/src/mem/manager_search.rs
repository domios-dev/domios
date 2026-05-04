//! Mem search and recall operations.

use super::{embeddings, layers, rerank, search, MemManager, SearchHit, SearchQuery, WakeUpOptions};

impl MemManager {
    /// Hybrid recall: RRF fusion of FTS5 + vector ANN results.
    /// Always runs both backends and merges via Reciprocal Rank Fusion
    /// for maximum recall quality (typo-tolerant + keyword-precise).
    /// Falls back to FTS5-only when embeddings feature is off.
    pub fn recall(&self, q: &SearchQuery) -> Result<Vec<SearchHit>, String> {
        let conn = self.conn.lock();
        search::hybrid_rrf(&conn, q, &self.blobs_dir)
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
}
