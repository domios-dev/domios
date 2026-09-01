//! Repository watcher — the event source behind status-bubble invalidation.
//!
//! One owning watcher per canonical worktree identity, refcounted by the
//! registry; the shared ref store (`commondir`) is refcounted independently so
//! two linked worktrees of one repository share a single `refs/` watch.
//! Events are precise: only `HEAD`/`index`, `refs/`/`packed-refs`, and
//! non-ignored working-tree paths reach a subscriber. There is no timer,
//! poll interval, or periodic refresh anywhere in this module.

mod backend;
mod events;
mod filter;
mod handles;
mod identity;
mod registry;

#[cfg(test)]
mod tests;

pub use backend::{ActiveWatch, BatchHandler, NotifyBackend, WatchBackend, WatchSpec, DEBOUNCE};
pub use events::{RepoEventSink, RepoWatchError, RepoWatchUnavailable, TauriRepoEventSink};
pub use handles::open_handle_count;
pub use identity::RepoIdentity;
pub use registry::{RepoWatchRegistry, RepoWatchRegistryHandle, RepoWatchSubscription};
