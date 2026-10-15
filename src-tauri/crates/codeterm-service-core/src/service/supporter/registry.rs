pub use codeterm_recall::registry::*;

use crate::mem::{
    rerank::RerankConfig,
    search::{SearchConfig, SearchQuery},
    SearchHit,
};
use crate::service::ServiceContext;
use std::sync::Arc;
use std::time::Instant;

impl DecisionBackend for ServiceContext {
    fn supporter_runtime(&self) -> Arc<crate::service::supporter::SupporterRuntime> {
        crate::service::supporter::runtime_with_notifications(self)
    }
    fn recall_untouched(
        &self,
        q: &SearchQuery,
        cfg: &SearchConfig,
    ) -> Result<Vec<SearchHit>, String> {
        self.mem.recall_with_config_untouched(q, cfg)
    }

    fn legacy_rerank_untouched(
        &self,
        q: &SearchQuery,
        cfg: &RerankConfig,
    ) -> Result<Vec<SearchHit>, String> {
        self.mem.search_with_rerank_untouched(q, cfg)
    }

    fn graph_candidates_untouched(
        &self,
        q: &SearchQuery,
        m: usize,
    ) -> Result<Vec<SearchHit>, String> {
        self.mem.graph_candidates_untouched(q, m)
    }

    fn session_candidates(
        &self,
        query: &str,
        limit: usize,
        max_body_chars: usize,
        deadline: Instant,
    ) -> Vec<sessions_source::SessionCandidate> {
        let registry = self.agent_registry.clone();
        sessions_source::search(
            move |opts| {
                registry
                    .search_sessions_with_stats(opts)
                    .map_err(|error| error.to_string())
            },
            query,
            limit,
            max_body_chars,
            deadline,
        )
    }

    fn record_access(&self, ids: &[String]) -> Result<(), String> {
        self.mem.record_access(ids)
    }
}
