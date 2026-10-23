use std::cell::RefCell;
use std::time::{Duration, Instant};

pub const QUERY_BUDGET: Duration = Duration::from_millis(500);

#[derive(Clone, Copy)]
struct RecallState {
    started: Instant,
    degraded: bool,
}

thread_local! {
    static RECALL: RefCell<Option<RecallState>> = const { RefCell::new(None) };
}

pub fn with_recall_status<T>(run: impl FnOnce() -> T) -> (T, bool) {
    struct Restore(Option<RecallState>);
    impl Drop for Restore {
        fn drop(&mut self) {
            RECALL.with(|state| *state.borrow_mut() = self.0);
        }
    }
    let _restore = Restore(RECALL.with(|state| {
        state.replace(Some(RecallState {
            started: Instant::now(),
            degraded: false,
        }))
    }));
    let result = run();
    let degraded = RECALL.with(|state| state.borrow().is_some_and(|state| state.degraded));
    (result, degraded)
}

pub(crate) fn remaining() -> Duration {
    remaining_at(Instant::now())
}

fn remaining_at(now: Instant) -> Duration {
    RECALL.with(|state| {
        state.borrow().map_or(QUERY_BUDGET, |state| {
            QUERY_BUDGET.saturating_sub(now.saturating_duration_since(state.started))
        })
    })
}

pub(crate) fn record<T>(result: Option<T>) -> Option<T> {
    if result.is_none() {
        RECALL.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.degraded = true;
            }
        });
    }
    result
}

pub(crate) fn wait<T>(budget: Duration, receive: impl FnOnce(Duration) -> Option<T>) -> Option<T> {
    if budget.is_zero() {
        return None;
    }
    receive(budget)
}

#[cfg(test)]
mod tests;
