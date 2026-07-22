//! Ownership handle proving a General Agent launch is actually running.
//!
//! A pane latched at a launch stage (`shell_ready`, `agent_launching`, …) with
//! no live owner is stale state, not progress — retries must not be refused as
//! "already in progress" forever. Held for the whole synchronous launch, so the
//! answer is an ownership fact rather than an elapsed-time guess.

use std::sync::atomic::{AtomicBool, Ordering};

static LAUNCH_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Released on drop, including on an early `return` out of the launch body.
pub struct LaunchGuard(());

impl LaunchGuard {
    /// `None` when another launch already owns the General Agent.
    pub fn acquire() -> Option<Self> {
        LAUNCH_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self(()))
    }
}

impl Drop for LaunchGuard {
    fn drop(&mut self) {
        LAUNCH_ACTIVE.store(false, Ordering::Release);
    }
}

pub fn launch_in_progress() -> bool {
    LAUNCH_ACTIVE.load(Ordering::Acquire)
}

/// The guard is process-global, so every test that touches it takes this lock —
/// otherwise parallel tests observe each other's ownership.
#[cfg(test)]
pub(crate) static TEST_SERIAL: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_is_exclusive_and_released_on_drop() {
        let _serial = TEST_SERIAL.lock();
        let guard = LaunchGuard::acquire().expect("first acquire");
        assert!(launch_in_progress());
        assert!(
            LaunchGuard::acquire().is_none(),
            "a second launch must not claim the General Agent"
        );

        drop(guard);
        assert!(!launch_in_progress());
        assert!(LaunchGuard::acquire().is_some(), "guard must be reusable");
    }
}
