use super::{Core, RetainedResources, Slot};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, TryLockError};

pub const WORKERS: usize = 2;

pub struct CaptureGuard {
    resources: Mutex<Option<Box<dyn RetainedResources>>>,
    claimed: AtomicBool,
}

pub fn retain(core: &Arc<Core>, index: Option<usize>, resources: Box<dyn RetainedResources>) {
    let capture = Arc::new(CaptureGuard {
        resources: Mutex::new(Some(resources)),
        claimed: AtomicBool::new(false),
    });
    {
        let mut state = core.state.lock().unwrap_or_else(|error| error.into_inner());
        state.captures.push(capture.clone());
        if let Some(index) = index {
            state.slots[index] = Slot::default();
        }
        core.changed.notify_all();
    }
}

pub fn owner_loop(core: Arc<Core>) {
    loop {
        let capture = {
            let mut state = core.state.lock().unwrap_or_else(|error| error.into_inner());
            loop {
                if let Some(capture) = state.captures.iter().find(|capture| {
                    capture
                        .claimed
                        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok()
                }) {
                    break capture.clone();
                }
                if state.closed && state.cleanup_complete() {
                    return;
                }
                state = core
                    .changed
                    .wait(state)
                    .unwrap_or_else(|error| error.into_inner());
            }
        };
        settle(&core, capture);
    }
}

fn settle(core: &Core, capture: Arc<CaptureGuard>) {
    let mut owned = capture
        .resources
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let Some(resources) = owned.as_mut() else {
        return;
    };
    for error in resources.request_cancel() {
        super::push_diagnostic(core, &error);
    }
    let settlement = resources.wait_for_capture_settlement();
    let settled = !settlement.remains_unconfirmed;
    super::settlement_diagnostics(core, settlement);
    if settled {
        finish(core, &capture, &mut owned);
    }
}

pub fn reclaim(core: &Arc<Core>) {
    let captures = core
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .captures
        .clone();
    for capture in captures {
        let mut owned = match capture.resources.try_lock() {
            Ok(owned) => owned,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => continue,
        };
        if owned
            .as_mut()
            .is_some_and(|resources| resources.try_settle())
        {
            finish(core, &capture, &mut owned);
        }
    }
}

fn finish(
    core: &Core,
    capture: &Arc<CaptureGuard>,
    owned: &mut Option<Box<dyn RetainedResources>>,
) {
    drop(owned.take());
    let mut state = core.state.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(index) = state
        .captures
        .iter()
        .position(|current| Arc::ptr_eq(current, capture))
    {
        state.captures.swap_remove(index);
        state.settled_total = state.settled_total.saturating_add(1);
    }
    core.changed.notify_all();
}
