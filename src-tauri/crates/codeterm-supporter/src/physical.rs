use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use codeterm_decision::{deadline, physical, DecisionError};
use parking_lot::Mutex;

use super::{SupporterRuntime, VerdictKind};

pub(super) struct Meter {
    runtime: Arc<SupporterRuntime>,
    cap: usize,
    kind: VerdictKind,
    tab: Option<String>,
    adapter: String,
    questions: usize,
    deadline: Instant,
    limit: Option<usize>,
    active: AtomicBool,
    state: Mutex<(usize, Option<DecisionError>)>,
}

impl Meter {
    pub(super) fn new(
        runtime: Arc<SupporterRuntime>,
        cap: usize,
        kind: VerdictKind,
        tab: Option<String>,
        adapter: String,
        questions: usize,
        deadline: Instant,
        limit: Option<usize>,
    ) -> Arc<Self> {
        Arc::new(Self {
            runtime,
            cap,
            kind,
            tab,
            adapter,
            questions,
            deadline,
            limit,
            active: AtomicBool::new(true),
            state: Mutex::new((0, None)),
        })
    }

    pub(super) fn finish(&self) -> Option<DecisionError> {
        self.active.store(false, Ordering::Release);
        self.state.lock().1.clone()
    }
}

impl physical::RequestObserver for Meter {
    fn deadline(&self) -> Option<Instant> {
        Some(self.deadline)
    }

    fn before_request(&self) -> Result<(), DecisionError> {
        let mut state = self.state.lock();
        let result = if !self.active.load(Ordering::Acquire) || deadline::now() >= self.deadline {
            Err(DecisionError::Deadline)
        } else if self.limit.is_some_and(|limit| state.0 >= limit) {
            Err(DecisionError::Unsupported)
        } else if state.0 > 0 {
            self.runtime.admit_adapter(&self.adapter)
        } else {
            self.runtime.reserve_request(
                self.cap,
                self.kind,
                self.tab.as_deref(),
                self.questions,
                &self.adapter,
            )
        };
        match &result {
            Ok(()) => state.0 += 1,
            Err(error) => {
                state.1.get_or_insert_with(|| error.clone());
            }
        }
        result
    }
}

pub(super) struct ActiveMeter(pub(super) Arc<Meter>);

impl Drop for ActiveMeter {
    fn drop(&mut self) {
        self.0.finish();
    }
}
