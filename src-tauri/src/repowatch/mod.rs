//! Repository watcher — the event source behind status-bubble invalidation. One
//! refcounted watcher per identity; the shared ref store refcounts separately.

mod backend;
mod events;
mod filter;
mod handles;
mod identity;
mod registry;

#[cfg(test)]
mod tests;

pub use backend::{
    is_rescan, ActiveWatch, BatchHandler, NotifyBackend, RescanRelay, RescanSink, WatchBackend,
    WatchSignal, WatchSpec, DEBOUNCE,
};
pub use events::{RepoEventSink, RepoWatchError, RepoWatchUnavailable, TauriRepoEventSink};
pub use handles::open_handle_count;
pub use identity::RepoIdentity;
pub use registry::{RepoWatchRegistry, RepoWatchRegistryHandle, RepoWatchSubscription};
