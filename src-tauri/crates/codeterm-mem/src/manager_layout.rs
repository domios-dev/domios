//! MemManager facade for the graph layout subsystem.

use std::sync::Arc;

use super::graph_layout::cache::LayoutCache;
use super::graph_layout::{project, snapshot};
use super::MemManager;

#[cfg(feature = "embeddings")]
const REBUILD_MISSING_RATIO: f64 = 0.20;

impl MemManager {
    pub fn layout_cache(&self) -> Arc<LayoutCache> {
        self.layout.cache.clone()
    }

    pub fn layout_is_rebuilding(&self) -> bool {
        self.layout.is_rebuilding()
    }

    /// Run the full projection synchronously (caller owns the background
    /// thread). Returns the new layout_version. Refuses concurrent rebuilds.
    pub fn run_full_layout(&self) -> Result<u64, String> {
        if !self.layout.try_begin_rebuild() {
            return Err("layout rebuild already running".into());
        }
        let cache = self.layout.cache.clone();
        let result = {
            let conn = self.conn.lock();
            project::run_full(&conn, &cache)
        };
        self.layout.end_rebuild();
        result
    }

    pub fn layout_snapshot(&self) -> Result<snapshot::Snapshot, String> {
        let cache = self.layout.cache.clone();
        let conn = self.conn.lock();
        snapshot::build_snapshot(&conn, &cache)
    }

    pub fn layout_delta(&self, since_seq: u64) -> Result<snapshot::DeltaResponse, String> {
        let cache = self.layout.cache.clone();
        let conn = self.conn.lock();
        snapshot::build_delta(&conn, &cache, since_seq)
    }

    pub fn layout_labels(&self, ids: &[String]) -> Result<Vec<(String, String)>, String> {
        let conn = self.conn.lock();
        snapshot::load_labels(&conn, ids)
    }

    pub fn layout_edges(&self, ids: &[String]) -> Result<Vec<snapshot::EdgeRow>, String> {
        let conn = self.conn.lock();
        snapshot::load_edges(&conn, ids)
    }

    /// Flush the layout cache if its debounce window elapsed. Called from the
    /// periodic background tick.
    pub fn layout_flush_if_due(&self) -> bool {
        self.layout.cache.flush_if_due()
    }

    /// Decide whether a full rebuild is warranted: cache empty but vectors
    /// exist, or too many vectorized items lack a position.
    #[cfg(feature = "embeddings")]
    pub fn layout_needs_rebuild(&self) -> bool {
        let cache = self.layout.cache.clone();
        let conn = self.conn.lock();
        let vectors = match project::load_primary_vectors(&conn) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if vectors.is_empty() {
            return false;
        }
        if cache.is_empty() {
            return true;
        }
        let missing = vectors
            .iter()
            .filter(|(id, _)| cache.get(id).is_none())
            .count();
        (missing as f64 / vectors.len() as f64) > REBUILD_MISSING_RATIO
    }

    #[cfg(not(feature = "embeddings"))]
    pub fn layout_needs_rebuild(&self) -> bool {
        false
    }

    /// Incrementally place one (re)embedded item from its raw vector. No-op
    /// when the cache has no base layout yet (a full rebuild will place it).
    #[cfg(feature = "embeddings")]
    pub fn layout_place_incremental(&self, mem_id: &str, vector: &[f32]) {
        if self.layout.cache.is_empty() {
            return;
        }
        let cache = self.layout.cache.clone();
        let conn = self.conn.lock();
        match super::graph_layout::placement::place_incremental(&conn, &cache, mem_id, vector) {
            Ok(true) => log::debug!("[graph_layout] placed {mem_id} incrementally"),
            Ok(false) => log::debug!("[graph_layout] no neighbors to place {mem_id}"),
            Err(e) => log::warn!("[graph_layout] place {mem_id} failed: {e}"),
        }
    }

    #[cfg(not(feature = "embeddings"))]
    pub fn layout_place_incremental(&self, _mem_id: &str, _vector: &[f32]) {}
}
