//! Commit authority for extraction generations.
//!
//! An extraction Worker receives a *copy* of its source (title + body) before
//! it spawns, so deleting the source row revokes nothing it holds. Authority
//! therefore lives here, not in the Worker: deleting a source revokes its open
//! generations, and the storage boundary refuses any commit carrying a revoked
//! generation — including one whose model execution already began.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// Identity of one extraction run: the exact source id, the row `uid` the copy
/// was taken from, and a monotonic sequence. `seq` is what makes a re-extraction
/// of the same source a *distinct* generation, so resolution can key on identity
/// and generation together instead of degrading to "the latest run for this id".
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GenerationId {
    pub source_id: String,
    pub source_uid: String,
    pub seq: u64,
}

impl GenerationId {
    /// Stable token for logs and honest cleanup reporting.
    pub fn token(&self) -> String {
        format!("{}#{}", self.source_id, self.seq)
    }
}

/// Outcome of resolving a write to a generation. `Unattributed` is the honest
/// answer for writes that belong to no tracked extraction run (user edits,
/// dreaming sweeps); it is never a guess at which generation was meant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitAuthority {
    Unattributed,
    Authorized(GenerationId),
    Revoked(GenerationId),
}

impl CommitAuthority {
    pub fn is_revoked(&self) -> bool {
        matches!(self, Self::Revoked(_))
    }

    /// Error text for a refused commit. Names the generation so the refusal is
    /// traceable to the deletion that caused it.
    pub fn refusal(&self) -> Option<String> {
        match self {
            Self::Revoked(generation) => Some(format!(
                "extraction generation {} was revoked: source {} is deleted",
                generation.token(),
                generation.source_id
            )),
            _ => None,
        }
    }
}

/// Result of attaching a Worker id to a generation. `AlreadyRevoked` means the
/// source was deleted between `open` and the Worker becoming known — the caller
/// cancels immediately on this value rather than waiting for anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindOutcome {
    Active,
    AlreadyRevoked,
}

/// A generation that lost its authority, paired with the Worker holding it (if
/// one was bound yet) so the exact Worker — and only that Worker — is cancelled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevokedGeneration {
    pub generation: GenerationId,
    pub worker_id: Option<String>,
}

/// Cancellation seam. `codeterm-mem` owns the authority but cannot close tabs;
/// the app layer registers a sink that kills exactly the listed Workers.
pub trait ExtractionCancelSink: Send + Sync {
    fn cancel(&self, revoked: &[RevokedGeneration]);
}

#[derive(Default)]
struct State {
    next_seq: u64,
    open: HashMap<GenerationId, Option<String>>,
    revoked: HashMap<GenerationId, Option<String>>,
    by_worker: HashMap<String, GenerationId>,
}

#[derive(Default)]
pub struct ExtractionAuthority {
    state: RwLock<State>,
    sink: RwLock<Option<Arc<dyn ExtractionCancelSink>>>,
}

impl ExtractionAuthority {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_cancel_sink(&self, sink: Arc<dyn ExtractionCancelSink>) {
        *self.sink.write() = Some(sink);
    }

    /// Allocate a generation for `source_id` taken from row `source_uid`.
    pub fn open(&self, source_id: &str, source_uid: &str) -> GenerationId {
        let mut st = self.state.write();
        st.next_seq += 1;
        let generation = GenerationId {
            source_id: source_id.to_string(),
            source_uid: source_uid.to_string(),
            seq: st.next_seq,
        };
        st.open.insert(generation.clone(), None);
        generation
    }

    /// Bind the Worker that owns `generation`. Returns `AlreadyRevoked` when the source
    /// was deleted before the Worker id existed, so the spawn site can cancel it
    /// without polling for the revocation it just missed.
    pub fn bind_worker(&self, generation: &GenerationId, worker_id: &str) -> BindOutcome {
        let mut st = self.state.write();
        st.by_worker
            .insert(worker_id.to_string(), generation.clone());
        if let Some(slot) = st.revoked.get_mut(generation) {
            *slot = Some(worker_id.to_string());
            return BindOutcome::AlreadyRevoked;
        }
        if let Some(slot) = st.open.get_mut(generation) {
            *slot = Some(worker_id.to_string());
        }
        BindOutcome::Active
    }

    /// Revoke every open generation whose source is exactly `source_id`, and
    /// notify the cancel sink. Generations for other sources are untouched — a
    /// delete must never widen into a sweep over unrelated extraction Workers.
    pub fn revoke_source(&self, source_id: &str) -> Vec<RevokedGeneration> {
        let revoked = {
            let mut st = self.state.write();
            let hit: Vec<GenerationId> = st
                .open
                .keys()
                .filter(|g| g.source_id == source_id)
                .cloned()
                .collect();
            let mut out = Vec::with_capacity(hit.len());
            for generation in hit {
                let worker_id = st.open.remove(&generation).flatten();
                st.revoked.insert(generation.clone(), worker_id.clone());
                out.push(RevokedGeneration {
                    generation,
                    worker_id,
                });
            }
            out.sort_by(|a, b| a.generation.cmp(&b.generation));
            out
        };
        if !revoked.is_empty() {
            let sink = self.sink.read().clone();
            if let Some(sink) = sink {
                sink.cancel(&revoked);
            }
        }
        revoked
    }

    /// Resolve a write to its generation by exact Worker identity. An unknown
    /// Worker is `Unattributed` — never resolved to the newest generation for
    /// some source, which is the latest-or-global bug class this guards against.
    pub fn authorize_worker(&self, worker_id: Option<&str>) -> CommitAuthority {
        let Some(worker_id) = worker_id else {
            return CommitAuthority::Unattributed;
        };
        let st = self.state.read();
        let Some(generation) = st.by_worker.get(worker_id) else {
            return CommitAuthority::Unattributed;
        };
        if st.revoked.contains_key(generation) {
            CommitAuthority::Revoked(generation.clone())
        } else {
            CommitAuthority::Authorized(generation.clone())
        }
    }

    /// Release a generation that finished on its own. Revoked generations stay
    /// recorded so a late commit from one is still refused.
    pub fn close(&self, generation: &GenerationId) {
        let mut st = self.state.write();
        st.open.remove(generation);
    }

    pub fn is_revoked(&self, generation: &GenerationId) -> bool {
        self.state.read().revoked.contains_key(generation)
    }

    pub fn open_generations(&self, source_id: &str) -> Vec<GenerationId> {
        let mut out: Vec<GenerationId> = self
            .state
            .read()
            .open
            .keys()
            .filter(|g| g.source_id == source_id)
            .cloned()
            .collect();
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests;
