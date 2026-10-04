use super::lock;
use super::support::Faults;
use crate::assets::{self, AssetId, CleanupReport, Layout};
use crate::error::StorageError;
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct PinState {
    next: u64,
    floors: BTreeMap<u64, u64>,
    wake_at: Option<u64>,
}

impl PinState {
    fn oldest(&self) -> u64 {
        self.floors.values().copied().min().unwrap_or(u64::MAX)
    }
}

/// Floors of open read snapshots. A released asset file is removed only after every
/// snapshot that could still see its committed reference has ended.
#[derive(Default)]
pub(super) struct ReadPins {
    state: Mutex<PinState>,
}

impl ReadPins {
    /// Loads the published revision under the pin lock. The writer publishes before it
    /// checks pins, so a floor never exceeds the snapshot its reader opens afterwards.
    pub(super) fn enter(&self, published: &AtomicU64) -> u64 {
        let mut state = lock(&self.state);
        let floor = published.load(Ordering::Acquire);
        let token = state.next;
        state.next += 1;
        state.floors.insert(token, floor);
        token
    }

    /// True when deferred removals became unpinned and the writer must be woken.
    pub(super) fn leave(&self, token: u64) -> bool {
        let mut state = lock(&self.state);
        state.floors.remove(&token);
        let oldest = state.oldest();
        if state.wake_at.is_some_and(|at| oldest >= at) {
            state.wake_at = None;
            return true;
        }
        false
    }

    /// True when no open snapshot predates `released`; otherwise asks the last such reader
    /// to wake the writer.
    fn clear_or_wake(&self, released: u64) -> bool {
        let mut state = lock(&self.state);
        if state.oldest() >= released {
            return true;
        }
        state.wake_at = Some(state.wake_at.map_or(released, |at| at.min(released)));
        false
    }
}

/// Released assets whose files await removal, with the revision that released each.
pub(crate) struct ReleaseQueue {
    pins: Arc<ReadPins>,
    deferred: Vec<(AssetId, u64)>,
}

impl ReleaseQueue {
    pub(super) fn new(pins: Arc<ReadPins>) -> Self {
        ReleaseQueue {
            pins,
            deferred: Vec::new(),
        }
    }

    pub(crate) fn defer(&mut self, released: impl IntoIterator<Item = (AssetId, u64)>) {
        for (asset, revision) in released {
            if !self.deferred.iter().any(|(queued, _)| *queued == asset) {
                self.deferred.push((asset, revision));
            }
        }
    }

    /// Removes every queued file that no open snapshot can still read; the rest stay queued.
    pub(crate) fn drain(
        &mut self,
        conn: &Connection,
        layout: &Layout,
        faults: &mut Faults,
    ) -> Result<CleanupReport, StorageError> {
        let queued = std::mem::take(&mut self.deferred);
        let (ready, waiting): (Vec<_>, Vec<_>) = queued
            .into_iter()
            .partition(|(_, released)| self.pins.clear_or_wake(*released));
        self.deferred = waiting;
        let ready: Vec<AssetId> = ready.into_iter().map(|(asset, _)| asset).collect();
        let mut report = if ready.is_empty() {
            CleanupReport::default()
        } else {
            assets::remove_released(conn, layout, faults, &ready)?
        };
        report.deferred = self.deferred.len() as u64;
        Ok(report)
    }
}
