use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::active::{ActiveEmbedder, EmbedderEvent};
use super::provider::EmbedderIdentity;

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReembedState {
    #[default]
    Idle,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwitchPlan {
    pub items: u64,
    pub entities: u64,
    pub docs: u64,
    pub blueprints: u64,
}

impl SwitchPlan {
    pub fn total(&self) -> u64 {
        self.items + self.entities + self.docs + self.blueprints
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemProgress {
    pub id: String,
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReembedProgress {
    pub state: ReembedState,
    pub target: Option<EmbedderIdentity>,
    pub plan: SwitchPlan,
    pub total: u64,
    pub done: u64,
    pub failed: u64,
    pub started_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub current_item: Option<ItemProgress>,
}

/// Background re-embed progress. Every change is pushed to the embedder's
/// observers; nothing has to poll it.
pub struct ReembedTracker {
    clock: Arc<dyn Clock>,
    generation: AtomicU64,
    claimed: AtomicU64,
    progress: Mutex<ReembedProgress>,
    storage: Mutex<Option<PathBuf>>,
}

impl ReembedTracker {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            generation: AtomicU64::new(0),
            claimed: AtomicU64::new(0),
            progress: Mutex::new(ReembedProgress::default()),
            storage: Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> ReembedProgress {
        self.progress.lock().clone()
    }

    pub(crate) fn retain_local_models(&self, events: &ActiveEmbedder) {
        let progress = self.progress.lock();
        let target = (progress.state == ReembedState::Running)
            .then_some(progress.target.as_ref())
            .flatten();
        super::local::retain_models(&events.identity(), target);
    }

    pub fn is_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
            && self.progress.lock().state == ReembedState::Running
    }

    pub(crate) fn restore(&self, path: PathBuf) {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(mut progress) = serde_json::from_slice::<ReembedProgress>(&bytes) {
                if progress.state == ReembedState::Running {
                    progress.state = ReembedState::Cancelled;
                    progress.last_error = Some(
                        "re-embed interrupted by process restart; stamped shadow work will resume"
                            .into(),
                    );
                }
                *self.progress.lock() = progress;
            }
        }
        *self.storage.lock() = Some(path);
    }

    fn persist(&self, progress: &ReembedProgress) {
        if let Some(path) = self.storage.lock().as_ref() {
            let result = serde_json::to_vec(progress)
                .map_err(|error| error.to_string())
                .and_then(|bytes| {
                    codeterm_util::private_file::write_atomic(path, &bytes)
                        .map_err(|error| error.to_string())
                });
            if let Err(error) = result {
                log::warn!("[mem.embedder] progress persistence failed: {error}");
            }
        }
    }

    pub(crate) fn fail(&self, generation: u64, error: String, events: &ActiveEmbedder) {
        if self.generation.load(Ordering::SeqCst) != generation
            || self.claimed.load(Ordering::SeqCst) == generation
        {
            return;
        }
        let snapshot = {
            let mut progress = self.progress.lock();
            if progress.state != ReembedState::Running {
                return;
            }
            progress.state = ReembedState::Failed;
            log::warn!(
                "[mem.embedder] re-embed generation {generation} failed at {}/{}: {error}",
                progress.done,
                progress.total
            );
            progress.last_error = Some(error);
            progress.finished_at = Some(self.clock.now());
            progress.clone()
        };
        self.persist(&snapshot);
        events.emit(&EmbedderEvent::Progress(snapshot));
    }

    /// Stops the running build unless its publication already began.
    pub(crate) fn cancel(&self, reason: String, events: &ActiveEmbedder) -> bool {
        let snapshot = {
            let mut progress = self.progress.lock();
            let generation = self.generation.load(Ordering::SeqCst);
            if !matches!(progress.state, ReembedState::Running | ReembedState::Failed) {
                return true;
            }
            if self.claimed.load(Ordering::SeqCst) == generation {
                return false;
            }
            self.generation.fetch_add(1, Ordering::SeqCst);
            progress.state = ReembedState::Cancelled;
            progress.last_error = Some(reason);
            progress.finished_at = Some(self.clock.now());
            progress.clone()
        };
        self.persist(&snapshot);
        events.emit(&EmbedderEvent::Progress(snapshot));
        true
    }

    pub(crate) fn claim(&self, generation: u64) -> bool {
        let progress = self.progress.lock();
        let current = self.generation.load(Ordering::SeqCst) == generation
            && progress.state == ReembedState::Running;
        if current {
            self.claimed.store(generation, Ordering::SeqCst);
        }
        current
    }

    /// A unit that failed earlier in this run embedded on retry.
    pub(crate) fn recovered(&self, generation: u64) {
        let mut progress = self.progress.lock();
        if self.generation.load(Ordering::SeqCst) == generation
            && progress.state == ReembedState::Running
        {
            progress.failed = progress.failed.saturating_sub(1);
        }
    }

    /// Counts checkpointed units as done and adds units a resume pass re-embeds.
    pub(crate) fn resume(&self, generation: u64, done: u64, extra: u64, events: &ActiveEmbedder) {
        let snapshot = {
            let mut progress = self.progress.lock();
            if self.generation.load(Ordering::SeqCst) != generation
                || progress.state != ReembedState::Running
                || done + extra == 0
            {
                return;
            }
            progress.done += done;
            progress.total += extra;
            progress.updated_at = Some(self.clock.now());
            progress.clone()
        };
        self.persist(&snapshot);
        events.emit(&EmbedderEvent::Progress(snapshot));
    }

    pub(crate) fn start(
        &self,
        target: EmbedderIdentity,
        plan: SwitchPlan,
        events: &ActiveEmbedder,
    ) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let now = self.clock.now();
        let snapshot = {
            let mut progress = self.progress.lock();
            *progress = ReembedProgress {
                state: ReembedState::Running,
                target: Some(target),
                plan,
                total: plan.total(),
                started_at: Some(now),
                updated_at: Some(now),
                ..ReembedProgress::default()
            };
            progress.clone()
        };
        self.persist(&snapshot);
        events.emit(&EmbedderEvent::Progress(snapshot));
        generation
    }

    pub(crate) fn item_progress(
        &self,
        generation: u64,
        id: &str,
        done: usize,
        total: usize,
        events: &ActiveEmbedder,
    ) -> bool {
        let snapshot = {
            let mut progress = self.progress.lock();
            if self.generation.load(Ordering::SeqCst) != generation
                || progress.state != ReembedState::Running
            {
                return false;
            }
            progress.current_item = Some(ItemProgress {
                id: id.into(),
                done,
                total,
            });
            progress.updated_at = Some(self.clock.now());
            progress.clone()
        };
        self.persist(&snapshot);
        events.emit(&EmbedderEvent::Progress(snapshot));
        true
    }

    pub(crate) fn advance(
        &self,
        generation: u64,
        outcome: Result<(), String>,
        events: &ActiveEmbedder,
    ) -> bool {
        if !self.is_current(generation) {
            return false;
        }
        let now = self.clock.now();
        let snapshot = {
            let mut progress = self.progress.lock();
            if self.generation.load(Ordering::SeqCst) != generation
                || progress.state != ReembedState::Running
            {
                return false;
            }
            progress.current_item = None;
            progress.done += 1;
            if let Err(error) = outcome {
                progress.failed += 1;
                progress.last_error = Some(error);
            }
            progress.updated_at = Some(now);
            let step = (progress.total / 100).max(1);
            (progress.done % step == 0 || progress.done >= progress.total).then(|| progress.clone())
        };
        if let Some(snapshot) = snapshot {
            self.persist(&snapshot);
            events.emit(&EmbedderEvent::Progress(snapshot));
        }
        true
    }

    pub(crate) fn finish(&self, generation: u64, events: &ActiveEmbedder) {
        if !self.is_current(generation) {
            return;
        }
        let now = self.clock.now();
        let snapshot = {
            let mut progress = self.progress.lock();
            if self.generation.load(Ordering::SeqCst) != generation
                || progress.state != ReembedState::Running
            {
                return;
            }
            progress.current_item = None;
            progress.state = ReembedState::Completed;
            progress.updated_at = Some(now);
            progress.finished_at = Some(now);
            progress.clone()
        };
        self.persist(&snapshot);
        events.emit(&EmbedderEvent::Progress(snapshot));
    }
}
