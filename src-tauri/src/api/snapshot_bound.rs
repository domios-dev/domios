//! Concurrency bound for full-state multiplexer snapshots.
//!
//! `GET /api/mux/state` clones the whole multiplexer under the mux and tracker
//! locks and serializes it. A switch storm drove 1,028-1,050 of those at once and
//! the daemon aborted twice. The bound caps how many can be building at any
//! instant; anything over it is answered immediately rather than queued, because a
//! queue under this load is the same unbounded work with a longer fuse.
//!
//! What gets worse: a client over the bound gets `503` instead of a snapshot, so a
//! caller that does not handle the status sees a failed refetch rather than a slow
//! one and shows stale state until its next poll succeeds. That is the deliberate
//! trade — the alternative, coalescing waiters onto one in-flight snapshot, makes
//! every waiter as slow as the slowest build and fails them all together.

use std::sync::Arc;

use axum::http::StatusCode;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Full-state snapshots allowed to be building at the same instant.
pub const MAX_CONCURRENT_STATE_SNAPSHOTS: usize = 8;

/// The answer to a request over the bound. Asserted by constant, never by literal.
pub const SNAPSHOT_OVERLOAD_STATUS: StatusCode = StatusCode::SERVICE_UNAVAILABLE;
pub const SNAPSHOT_OVERLOAD_BODY: &str = "state snapshot capacity reached";

/// Held for exactly as long as one snapshot is being built.
pub type SnapshotPermit = OwnedSemaphorePermit;

#[derive(Clone)]
pub struct SnapshotBound {
    permits: Arc<Semaphore>,
    limit: usize,
}

impl SnapshotBound {
    pub fn new() -> Self {
        Self::with_limit(MAX_CONCURRENT_STATE_SNAPSHOTS)
    }

    pub fn with_limit(limit: usize) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(limit)),
            limit,
        }
    }

    /// `None` means the bound is reached. The caller answers; it never waits.
    pub fn try_enter(&self) -> Option<SnapshotPermit> {
        self.permits.clone().try_acquire_owned().ok()
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn in_flight(&self) -> usize {
        self.limit()
            .saturating_sub(self.permits.available_permits())
    }
}

impl Default for SnapshotBound {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
