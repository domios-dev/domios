use super::backend::{ActiveWatch, NotifyBackend, WatchBackend, WatchSignal, WatchSpec};
use super::events::{RepoEventSink, RepoWatchError, RepoWatchUnavailable};
use super::filter::{batch_is_relevant, WatchHalf};
use super::identity::RepoIdentity;
use super::unresolved::{PathShape, UnresolvedPaths};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
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
}

struct SharedEntry {
    refcount: usize,
    members: SharedMembers,
    watch: Box<dyn ActiveWatch>,
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
    lane_tokens: Mutex<HashMap<String, String>>,
    unresolved: Mutex<UnresolvedPaths>,
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
            lane_tokens: Mutex::new(HashMap::new()),
            unresolved: Mutex::new(UnresolvedPaths::default()),
            next_token: AtomicU64::new(1),
        }
    }

    /// Take a reference on the watch for this path's repository identity.
    /// Failure is loud on both channels: an error here and an unavailable event.
    /// A path already refused as no repository fails quietly until its shape changes.
    pub fn subscribe(&self, path: &Path) -> Result<RepoWatchSubscription, RepoWatchError> {
        let key = path_key(&path.to_string_lossy());
        let shape = PathShape::probe(path);
        if let Some(refusal) = self.unresolved.lock().refusal(&key, shape) {
            return Err(refusal);
        }
        let identity = match RepoIdentity::resolve(path) {
            Ok(identity) => {
                self.unresolved.lock().resolved(&key);
                identity
            }
            Err(e) => {
                self.unresolved.lock().refuse(key, shape, e.clone());
                self.sink.repository_unavailable(&e.identity, e.reason);
                return Err(e);
            }
        };

        let result: Result<RepoWatchSubscription, RepoWatchError> = (|| {
            let mut inner = self.inner.lock();
            self.retain_shared(&mut inner, &identity)?;
            inner
                .entries
                .entry(identity.key.clone())
                .or_insert(WorktreeEntry { refcount: 0 });
            if let Some(entry) = inner.entries.get_mut(&identity.key) {
                entry.refcount += 1;
            }

            let token = format!("rw_{}", self.next_token.fetch_add(1, Ordering::Relaxed));
            inner.tokens.insert(token.clone(), identity.clone());
            Ok(RepoWatchSubscription {
                token,
                identity: identity.id,
            })
        })();
        if let Err(error) = &result {
            self.sink
                .repository_unavailable(&error.identity, error.reason);
        }
        result
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
                if shared.refcount > 0 {
                    let members = shared.members.lock();
                    if let Err(detail) = shared.watch.update(&watch_specs(members.values())) {
                        log::warn!("[repowatch] removing worktree watch failed: {detail}");
                    }
                }
            }
            if shared.refcount == 0 {
                inner.shared.remove(&identity.common_key);
            }
        }
        true
    }

    pub fn retain_lane_watch(
        &self,
        lane_path: &str,
        canonical_path: &Path,
    ) -> Result<(), RepoWatchError> {
        let key = path_key(lane_path);
        if self.lane_tokens.lock().contains_key(&key) {
            return Ok(());
        }
        let subscription = self.subscribe(canonical_path)?;
        let mut lane_tokens = self.lane_tokens.lock();
        match lane_tokens.entry(key) {
            std::collections::hash_map::Entry::Occupied(_) => {
                drop(lane_tokens);
                self.release(&subscription.token);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(subscription.token);
            }
        }
        Ok(())
    }

    pub fn release_lane_watch(&self, lane_path: &str) -> bool {
        let token = self.lane_tokens.lock().remove(&path_key(lane_path));
        token.is_some_and(|token| self.release(&token))
    }

    fn retain_shared(
        &self,
        inner: &mut Inner,
        identity: &RepoIdentity,
    ) -> Result<(), RepoWatchError> {
        if let Some(entry) = inner.shared.get_mut(&identity.common_key) {
            let mut candidates = entry.members.lock().clone();
            let fresh = candidates
                .insert(identity.key.clone(), identity.clone())
                .is_none();
            if fresh {
                entry
                    .watch
                    .update(&watch_specs(candidates.values()))
                    .map_err(|detail| RepoWatchError::watch_refused(&identity.id, detail))?;
                *entry.members.lock() = candidates;
            }
            entry.refcount += 1;
            return Ok(());
        }
        let members: SharedMembers = Arc::new(Mutex::new(BTreeMap::from([(
            identity.key.clone(),
            identity.clone(),
        )])));
        let fanout = members.clone();
        let sink = self.sink.clone();
        let handler = Box::new(move |signal: WatchSignal| {
            let targets: Vec<_> = fanout.lock().values().cloned().collect();
            match signal {
                WatchSignal::Batch(paths) => {
                    for target in &targets {
                        let relevant = batch_is_relevant(target, WatchHalf::Worktree, &paths)
                            .and_then(|worktree| {
                                if worktree {
                                    Ok(true)
                                } else {
                                    batch_is_relevant(target, WatchHalf::SharedRefs, &paths)
                                }
                            });
                        match relevant {
                            Ok(true) => sink.repository_changed(&target.id),
                            Ok(false) => {}
                            Err(error) => {
                                sink.repository_unavailable(&error.identity, error.reason)
                            }
                        }
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
        let specs = watch_specs(std::iter::once(identity));
        let watch = self
            .backend
            .build(&specs, handler)
            .map_err(|detail| RepoWatchError::watch_refused(&identity.id, detail))?;
        inner.shared.insert(
            identity.common_key.clone(),
            SharedEntry {
                refcount: 1,
                members,
                watch,
            },
        );
        Ok(())
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

fn watch_specs<'a>(members: impl Iterator<Item = &'a RepoIdentity>) -> Vec<WatchSpec> {
    let mut specs = Vec::new();
    for identity in members {
        specs.push(WatchSpec::shallow(identity.gitdir.clone()));
        specs.push(WatchSpec::worktree(identity.workdir.clone()));
        if !specs.iter().any(|spec| spec.optional) {
            specs.push(WatchSpec::shallow(identity.common_dir.clone()));
            specs.push(WatchSpec::optional_recursive(
                identity.common_dir.join("refs"),
            ));
        }
    }
    specs.sort_by(|a, b| a.path.cmp(&b.path));
    specs.dedup();
    specs
}

fn path_key(path: &str) -> String {
    let path = path.trim().replace('\\', "/");
    if cfg!(windows) || cfg!(target_os = "macos") {
        path.to_lowercase()
    } else {
        path
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
        log::warn!(
            "[repowatch] {} untrusted ({}): {detail}",
            target.id,
            reason.as_str()
        );
        sink.repository_unavailable(&target.id, reason);
    }
}
