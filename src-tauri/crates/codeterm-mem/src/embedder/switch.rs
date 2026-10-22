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
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwitchPlan {
    pub items: u64,
    pub entities: u64,
    pub docs: u64,
}

impl SwitchPlan {
    pub fn total(&self) -> u64 {
        self.items + self.entities + self.docs
    }
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
}

/// Background re-embed progress. Every change is pushed to the embedder's
/// observers; nothing has to poll it.
pub struct ReembedTracker {
    clock: Arc<dyn Clock>,
    generation: AtomicU64,
    progress: Mutex<ReembedProgress>,
}

impl ReembedTracker {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            generation: AtomicU64::new(0),
            progress: Mutex::new(ReembedProgress::default()),
        }
    }

    pub fn snapshot(&self) -> ReembedProgress {
        self.progress.lock().clone()
    }

    pub fn is_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
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
        events.emit(&EmbedderEvent::Progress(snapshot));
        generation
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
            progress.state = ReembedState::Completed;
            progress.updated_at = Some(now);
            progress.finished_at = Some(now);
            progress.clone()
        };
        events.emit(&EmbedderEvent::Progress(snapshot));
    }
}
