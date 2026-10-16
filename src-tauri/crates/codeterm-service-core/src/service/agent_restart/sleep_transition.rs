use super::RestartTracker;
use crate::pty::screen_state::ScreenChangeSignal;
use parking_lot::Mutex;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct SleepTransition {
    completion: Mutex<Option<Result<(), String>>>,
    signal: ScreenChangeSignal,
    wake_requested: AtomicBool,
}

pub struct SleepTransitionClaim {
    tracker: RestartTracker,
    tab_id: String,
    transition: Arc<SleepTransition>,
}

pub fn claim(tracker: &RestartTracker, tab_id: &str) -> Result<SleepTransitionClaim, String> {
    let mut state = tracker.lock();
    if state.in_progress.contains(tab_id) {
        return Err("Tab already has a lifecycle transition in progress".into());
    }
    let transition = Arc::new(SleepTransition::default());
    state.in_progress.insert(tab_id.to_owned());
    state
        .sleep_transitions
        .insert(tab_id.to_owned(), transition.clone());
    Ok(SleepTransitionClaim {
        tracker: tracker.clone(),
        tab_id: tab_id.to_owned(),
        transition,
    })
}

pub(crate) struct SleepRetirement(Arc<SleepTransition>);

pub(crate) fn request_wake(tracker: &RestartTracker, tab_id: &str) -> Option<SleepRetirement> {
    let transition = tracker.lock().sleep_transitions.get(tab_id).cloned()?;
    transition.wake_requested.store(true, Ordering::Release);
    Some(SleepRetirement(transition))
}

pub(crate) fn wait_for_retirement(
    retirement: Option<SleepRetirement>,
    deadline: Instant,
) -> Result<(), String> {
    let Some(SleepRetirement(transition)) = retirement else {
        return Ok(());
    };
    loop {
        let generation = transition.signal.generation();
        if let Some(result) = transition.completion.lock().clone() {
            return result;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("Tab sleep retirement did not complete before the wake deadline".into());
        }
        transition.signal.wait_after(generation, remaining);
    }
}

impl SleepTransitionClaim {
    pub fn wake_requested(&self) -> bool {
        self.transition.wake_requested.load(Ordering::Acquire)
    }

    pub fn finish(self, result: Result<(), String>) {
        self.complete(result);
    }

    fn complete(&self, result: Result<(), String>) {
        let mut state = self.tracker.lock();
        if !state
            .sleep_transitions
            .get(&self.tab_id)
            .is_some_and(|current| Arc::ptr_eq(current, &self.transition))
        {
            return;
        }
        *self.transition.completion.lock() = Some(result);
        state.sleep_transitions.remove(&self.tab_id);
        state.in_progress.remove(&self.tab_id);
        drop(state);
        self.transition.signal.mark_changed();
    }
}

impl Drop for SleepTransitionClaim {
    fn drop(&mut self) {
        self.complete(Err("Tab sleep retirement was interrupted".into()));
    }
}

pub(crate) fn wake_deadline() -> Instant {
    Instant::now() + Duration::from_secs(super::RESTART_READY_TIMEOUT_SECS)
}

#[cfg(test)]
mod tests;
