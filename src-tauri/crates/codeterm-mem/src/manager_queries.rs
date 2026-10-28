//! Mem aggregate / read-only query operations.

use super::{seed, store, MemManager, MemStats};

impl MemManager {
    pub fn graph_full(&self) -> Result<store::GraphDump, String> {
        self.store.graph_full()
    }

    pub fn list_pinned_positions(&self) -> Result<Vec<store::PinnedPosition>, String> {
        self.store.list_pinned_positions()
    }

    pub fn list_pinned(&self, limit: usize) -> Result<Vec<store::MemItem>, String> {
        self.store.list_pinned(limit)
    }

    pub fn list_by_source_prefix(
        &self,
        kind: &str,
        prefix: &str,
    ) -> Result<Vec<store::MemItem>, String> {
        self.store.list_by_source_prefix(kind, prefix)
    }

    pub fn set_pinned_position(&self, mem_id: &str, x: f64, y: f64) -> Result<bool, String> {
        self.store
            .set_pinned_position(&Self::write_context(), mem_id, x, y)
    }

    pub fn delete_pinned_position(&self, mem_id: &str) -> Result<usize, String> {
        self.store
            .delete_pinned_position(&Self::write_context(), mem_id)
    }

    pub fn stats(&self) -> Result<MemStats, String> {
        self.store.stats()
    }

    /// Full per-item audit trail: every create/update/promote/invalidate/
    /// merge/delete event ever logged for this id, ordered chronologically.
    pub fn get_item_history(&self, id: &str) -> Result<Vec<store::HistoryEntry>, String> {
        self.store.get_item_history(id)
    }

    /// `{area -> {kind -> count}}` grouped by area (NULL -> "(none)") and
    /// kind. When `workspace` is Some, filter to rows whose scope matches
    /// `workspace:<id>`. When None, aggregate across all scopes.
    pub fn get_taxonomy(
        &self,
        workspace: Option<&str>,
    ) -> Result<std::collections::HashMap<String, std::collections::HashMap<String, u64>>, String>
    {
        self.store.get_taxonomy(workspace)
    }

    /// Recent mem writes (feeds `mem tail`). When `since` is Some, returns
    /// rows with `updated_at > since`. When None, returns the `limit` most
    /// recent rows.
    pub fn tail_recent(
        &self,
        since: Option<&str>,
        limit: usize,
    ) -> Result<Vec<store::TailEntry>, String> {
        self.store.tail_recent(since, limit)
    }

    /// Dream diaries created at or after `since_ms` (epoch millis), newest first.
    pub fn dream_diaries_since(&self, since_ms: u64) -> Result<Vec<String>, String> {
        self.store.dream_diaries_since(since_ms)
    }

    pub fn seed_bundled_if_needed(&self) -> Result<seed::SeedOutcome, String> {
        seed::seed_bundled(self)
    }
}
