#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::LazyLock;

#[cfg(test)]
static PROMPT_STEP_DISPATCHES: LazyLock<parking_lot::Mutex<HashMap<String, usize>>> =
    LazyLock::new(Default::default);

pub(super) fn write_prompt_steps(pty: crate::PtyState, pty_id: String, steps: Vec<Vec<u8>>) {
    dispatch_prompt_steps(pty, pty_id, steps, || true);
}

pub(in crate::service) fn write_bound_prompt_steps<F>(
    pty: crate::PtyState,
    pty_id: String,
    steps: Vec<Vec<u8>>,
    still_bound: F,
) where
    F: FnOnce() -> bool + Send + 'static,
{
    dispatch_prompt_steps(pty, pty_id, steps, still_bound);
}

fn dispatch_prompt_steps<F>(
    pty: crate::PtyState,
    pty_id: String,
    steps: Vec<Vec<u8>>,
    still_bound: F,
) where
    F: FnOnce() -> bool + Send + 'static,
{
    #[cfg(test)]
    record_prompt_step_dispatch_for_test(&pty_id);
    std::thread::spawn(move || {
        if !still_bound() {
            return;
        }
        for step in steps {
            let _ = pty.lock().write(&pty_id, &step);
        }
    });
}

#[cfg(test)]
pub(in crate::service) fn prompt_step_dispatch_count_for_test(pty_id: &str) -> usize {
    PROMPT_STEP_DISPATCHES
        .lock()
        .get(pty_id)
        .copied()
        .unwrap_or_default()
}

#[cfg(test)]
pub(in crate::service) fn record_prompt_step_dispatch_for_test(pty_id: &str) {
    *PROMPT_STEP_DISPATCHES
        .lock()
        .entry(pty_id.to_string())
        .or_default() += 1;
}
