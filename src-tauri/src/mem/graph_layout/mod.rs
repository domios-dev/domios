//! Backend graph layout: sidecar position cache, full 2D projection, kNN
//! incremental placement, and snapshot/delta read model. Gated behind the
//! `embeddings` feature (positions derive from vectors); a no-op stub keeps
//! the surface available when the feature is off.

pub mod cache;
pub mod placement;
pub mod project;
pub mod snapshot;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cache::LayoutCache;

/// Owns the in-memory layout cache and the "rebuild running" guard. Lives on
/// MemManager so route handlers and the embed hook can reach it.
pub struct LayoutState {
    pub cache: Arc<LayoutCache>,
    rebuild_running: AtomicBool,
}

impl LayoutState {
    pub fn load(mem_dir: &Path) -> Self {
        let path = mem_dir.join("graph_layout.json");
        Self {
            cache: Arc::new(LayoutCache::load(path)),
            rebuild_running: AtomicBool::new(false),
        }
    }

    /// Try to claim the rebuild slot. Returns false if a rebuild is in flight.
    pub fn try_begin_rebuild(&self) -> bool {
        self.rebuild_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub fn end_rebuild(&self) {
        self.rebuild_running.store(false, Ordering::SeqCst);
    }

    pub fn is_rebuilding(&self) -> bool {
        self.rebuild_running.load(Ordering::SeqCst)
    }
}
