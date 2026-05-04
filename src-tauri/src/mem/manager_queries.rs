//! Mem aggregate / read-only query operations.

use super::{seed, store, MemManager, MemStats};

impl MemManager {
    pub fn graph_full(&self) -> Result<store::GraphDump, String> {
        let conn = self.conn.lock();
        store::graph_full(&conn)
    }

    pub fn list_pinned_positions(&self) -> Result<Vec<store::PinnedPosition>, String> {
        let conn = self.conn.lock();
        store::list_pinned_positions(&conn)
    }

    pub fn set_pinned_position(&self, mem_id: &str, x: f64, y: f64) -> Result<bool, String> {
        let conn = self.conn.lock();
        store::set_pinned_position(&conn, mem_id, x, y)
    }

    pub fn delete_pinned_position(&self, mem_id: &str) -> Result<usize, String> {
        let conn = self.conn.lock();
        store::delete_pinned_position(&conn, mem_id)
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

    pub fn seed_bundled_if_needed(&self) -> Result<usize, String> {
        seed::seed_bundled(self)
    }
}
