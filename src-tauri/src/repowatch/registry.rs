use super::backend::{ActiveWatch, NotifyBackend, WatchBackend, WatchSignal, WatchSpec};
use super::events::{RepoEventSink, RepoWatchError, RepoWatchUnavailable};
use super::filter::{batch_is_relevant, WatchHalf};
use super::identity::RepoIdentity;
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub type RepoWatchRegistryHandle = Arc<RepoWatchRegistry>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoWatchSubscription {
    pub token: String,
    pub identity: String,
}

type SharedMembers = Arc<Mutex<BTreeMap<String, RepoIdentity>>>;

struct WorktreeEntry {
    refcount: usize,
    _watch: Box<dyn ActiveWatch>,
}

struct SharedEntry {
    refcount: usize,
    members: SharedMembers,
    _watch: Box<dyn ActiveWatch>,
}

#[derive(Default)]
struct Inner {
    entries: HashMap<String, WorktreeEntry>,
    shared: HashMap<String, SharedEntry>,
    tokens: HashMap<String, RepoIdentity>,
}

/// The sole owner of repository watches. No other code path opens one.
pub struct RepoWatchRegistry {
    sink: Arc<dyn RepoEventSink>,
    backend: Arc<dyn WatchBackend>,
    inner: Mutex<Inner>,
    next_token: AtomicU64,
}

impl RepoWatchRegistry {
    pub fn new(sink: Arc<dyn RepoEventSink>) -> Self {
        Self::with_backend(sink, Arc::new(NotifyBackend))
    }

    pub fn with_backend(sink: Arc<dyn RepoEventSink>, backend: Arc<dyn WatchBackend>) -> Self {
        Self {
            sink,
            backend,
            inner: Mutex::new(Inner::default()),
            next_token: AtomicU64::new(1),
        }
    }

    /// Take a reference on the watch for this path's repository identity.
    /// Failure is loud on both channels: an error here and an unavailable event.
    pub fn subscribe(&self, path: &Path) -> Result<RepoWatchSubscription, RepoWatchError> {
        let identity = match RepoIdentity::resolve(path) {
            Ok(identity) => identity,
            Err(e) => {
                self.sink.repository_unavailable(&e.identity, e.reason);
                return Err(e);
            }
        };

        let mut inner = self.inner.lock();
        let fresh_entry = !inner.entries.contains_key(&identity.key);
        if fresh_entry {
            let watch = self.build_worktree_watch(&identity)?;
            inner.entries.insert(
                identity.key.clone(),
                WorktreeEntry {
                    refcount: 0,
                    _watch: watch,
                },
            );
        }
        if let Err(e) = self.retain_shared(&mut inner, &identity) {
            if fresh_entry {
                inner.entries.remove(&identity.key);
            }
            return Err(e);
        }
        if let Some(entry) = inner.entries.get_mut(&identity.key) {
            entry.refcount += 1;
        }

        let token = format!("rw_{}", self.next_token.fetch_add(1, Ordering::Relaxed));
        inner.tokens.insert(token.clone(), identity.clone());
        Ok(RepoWatchSubscription {
            token,
            identity: identity.id,
        })
    }

    /// Release one reference. The last release for an identity drops its
    /// watcher AND its shared-ref membership — never an earlier one.
    pub fn release(&self, token: &str) -> bool {
        let mut inner = self.inner.lock();
        let Some(identity) = inner.tokens.remove(token) else {
            return false;
        };
        let mut identity_released = false;
        if let Some(entry) = inner.entries.get_mut(&identity.key) {
            entry.refcount = entry.refcount.saturating_sub(1);
            identity_released = entry.refcount == 0;
        }
        if identity_released {
            inner.entries.remove(&identity.key);
        }
        if let Some(shared) = inner.shared.get_mut(&identity.common_key) {
            shared.refcount = shared.refcount.saturating_sub(1);
            // Membership follows the identity's own watcher, not one token:
            // dropping it early would fan ref events out to nobody.
            if identity_released {
                shared.members.lock().remove(&identity.key);
            }
            if shared.refcount == 0 {
                inner.shared.remove(&identity.common_key);
            }
        }
        true
    }

    fn build_worktree_watch(
        &self,
        identity: &RepoIdentity,
    ) -> Result<Box<dyn ActiveWatch>, RepoWatchError> {
        // HEAD and index live in the gitdir; everything else under it is
        // filtered out, and the gitdir is never watched recursively.
        let specs = vec![
            WatchSpec::shallow(identity.gitdir.clone()),
            WatchSpec::recursive(identity.workdir.clone()),
        ];
        let owner = identity.clone();
        let sink = self.sink.clone();
        let handler = Box::new(move |signal: WatchSignal| match signal {
            WatchSignal::Batch(paths) => dispatch(&sink, &owner, WatchHalf::Worktree, &paths),
            WatchSignal::Rescan => untrusted(
                &sink,
                std::slice::from_ref(&owner),
                RepoWatchUnavailable::RescanRequired,
                "backend reported a rescan: events may have been missed",
            ),
            WatchSignal::Failed(detail) => untrusted(
                &sink,
                std::slice::from_ref(&owner),
                RepoWatchUnavailable::BackendFailed,
                &detail,
            ),
        });
        self.backend
            .build(&specs, handler)
            .map_err(|detail| self.refuse(&identity.id, detail))
    }

    fn retain_shared(
        &self,
        inner: &mut Inner,
        identity: &RepoIdentity,
    ) -> Result<(), RepoWatchError> {
        if let Some(entry) = inner.shared.get_mut(&identity.common_key) {
            entry.refcount += 1;
            entry
                .members
                .lock()
                .insert(identity.key.clone(), identity.clone());
            return Ok(());
        }
        let members: SharedMembers = Arc::new(Mutex::new(BTreeMap::new()));
        members
            .lock()
            .insert(identity.key.clone(), identity.clone());
        // Non-recursive on the commondir carries packed-refs; refs/ is watched
        // recursively because a loose ref lands several levels deep.
        let specs = vec![
            WatchSpec::shallow(identity.common_dir.clone()),
            WatchSpec::optional_recursive(identity.common_dir.join("refs")),
        ];
        let sink = self.sink.clone();
        let fanout = members.clone();
        let handler = Box::new(move |signal: WatchSignal| {
            let targets: Vec<RepoIdentity> = fanout.lock().values().cloned().collect();
            match signal {
                WatchSignal::Batch(paths) => {
                    for target in &targets {
                        dispatch(&sink, target, WatchHalf::SharedRefs, &paths);
                    }
                }
                WatchSignal::Rescan => untrusted(
                    &sink,
                    &targets,
                    RepoWatchUnavailable::RescanRequired,
                    "backend reported a rescan: events may have been missed",
                ),
                WatchSignal::Failed(detail) => untrusted(
                    &sink,
                    &targets,
                    RepoWatchUnavailable::BackendFailed,
                    &detail,
                ),
            }
        });
        let watch = self
            .backend
            .build(&specs, handler)
            .map_err(|detail| self.refuse(&identity.id, detail))?;
        inner.shared.insert(
            identity.common_key.clone(),
            SharedEntry {
                refcount: 1,
                members,
                _watch: watch,
            },
        );
        Ok(())
    }

    fn refuse(&self, identity: &str, detail: String) -> RepoWatchError {
        let error = RepoWatchError::watch_refused(identity, detail);
        self.sink
            .repository_unavailable(&error.identity, RepoWatchUnavailable::WatchRefused);
        error
    }

    pub fn entry_count(&self) -> usize {
        self.inner.lock().entries.len()
    }

    pub fn shared_count(&self) -> usize {
        self.inner.lock().shared.len()
    }

    pub fn refcount(&self, key: &str) -> usize {
        self.inner
            .lock()
            .entries
            .get(key)
            .map(|e| e.refcount)
            .unwrap_or(0)
    }

    pub fn shared_refcount(&self, common_key: &str) -> usize {
        self.inner
            .lock()
            .shared
            .get(common_key)
            .map(|e| e.refcount)
            .unwrap_or(0)
    }

    pub fn shared_member_count(&self, common_key: &str) -> usize {
        self.inner
            .lock()
            .shared
            .get(common_key)
            .map(|e| e.members.lock().len())
            .unwrap_or(0)
    }
}

fn dispatch(
    sink: &Arc<dyn RepoEventSink>,
    identity: &RepoIdentity,
    half: WatchHalf,
    paths: &[PathBuf],
) {
    match batch_is_relevant(identity, half, paths) {
        Ok(true) => sink.repository_changed(&identity.id),
        Ok(false) => {}
        Err(e) => sink.repository_unavailable(&e.identity, e.reason),
    }
}

/// The watch can no longer be trusted. Never rescan, retry or synthesize a
/// change here: say so and let the UI show an honest unknown.
fn untrusted(
    sink: &Arc<dyn RepoEventSink>,
    targets: &[RepoIdentity],
    reason: RepoWatchUnavailable,
    detail: &str,
) {
    for target in targets {
        log::warn!("[repowatch] {} untrusted ({}): {detail}", target.id, reason.as_str());
        sink.repository_unavailable(&target.id, reason);
    }
}
