use std::sync::{Arc, LazyLock, Weak};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownReason {
    UserExit,
    Update,
    Restart,
    Signal,
}

static SENTINEL_SHUTDOWN: LazyLock<ShutdownLatch> = LazyLock::new(ShutdownLatch::default);

type ShutdownCallback = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct SubscriptionState {
    reason: Option<ShutdownReason>,
    next_id: u64,
    callback: Option<(u64, ShutdownCallback)>,
}

#[derive(Clone, Default)]
pub struct ShutdownLatch {
    state: Arc<Mutex<SubscriptionState>>,
}

pub struct ShutdownSubscription {
    state: Weak<Mutex<SubscriptionState>>,
    id: u64,
}

impl ShutdownLatch {
    pub fn subscribe(
        &self,
        callback: impl FnOnce() + Send + 'static,
    ) -> Result<ShutdownSubscription, &'static str> {
        let mut callback: Option<ShutdownCallback> = Some(Box::new(callback));
        let (id, armed) = {
            let mut state = self.state.lock();
            if state.callback.is_some() {
                return Err("Sentinel shutdown subscription already occupied");
            }
            state.next_id = state
                .next_id
                .checked_add(1)
                .ok_or("Sentinel shutdown subscription identity exhausted")?;
            let id = state.next_id;
            let armed = state.reason.is_some();
            if !armed {
                state.callback = callback.take().map(|callback| (id, callback));
            }
            (id, armed)
        };
        if armed {
            if let Some(callback) = callback {
                callback();
            }
        }
        Ok(ShutdownSubscription {
            state: Arc::downgrade(&self.state),
            id,
        })
    }

    pub fn arm(&self, reason: ShutdownReason) {
        let callback = {
            let mut state = self.state.lock();
            state.reason.get_or_insert(reason);
            state.callback.take().map(|(_, callback)| callback)
        };
        if let Some(callback) = callback {
            callback();
        }
    }
}

impl Drop for ShutdownSubscription {
    fn drop(&mut self) {
        if let Some(state) = self.state.upgrade() {
            let mut state = state.lock();
            if state
                .callback
                .as_ref()
                .is_some_and(|(id, _)| *id == self.id)
            {
                state.callback = None;
            }
        }
    }
}

pub fn subscribe_sentinel_shutdown(
    callback: impl FnOnce() + Send + 'static,
) -> Result<ShutdownSubscription, &'static str> {
    SENTINEL_SHUTDOWN.subscribe(callback)
}

pub fn arm_sentinel_shutdown(reason: ShutdownReason) {
    SENTINEL_SHUTDOWN.arm(reason);
}

#[cfg(test)]
mod subscription_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Barrier;

    #[test]
    fn sentinel_command_shutdown_registration_before_after_and_repeated_reasons() {
        for before in [true, false] {
            let latch = ShutdownLatch::default();
            let calls = Arc::new(AtomicUsize::new(0));
            if !before {
                latch.arm(ShutdownReason::Update);
            }
            let observed = calls.clone();
            let checking = latch.clone();
            let _subscription = latch
                .subscribe(move || {
                    assert!(
                        checking.state.try_lock().is_some(),
                        "callback cannot run under registry lock"
                    );
                    observed.fetch_add(1, Ordering::SeqCst);
                })
                .unwrap();
            latch.arm(ShutdownReason::Update);
            latch.arm(ShutdownReason::Signal);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(latch.state.lock().reason, Some(ShutdownReason::Update));
        }
    }

    #[test]
    fn sentinel_command_shutdown_concurrent_registration_and_arm_deliver_once() {
        for _ in 0..32 {
            let latch = ShutdownLatch::default();
            let barrier = Arc::new(Barrier::new(2));
            let calls = Arc::new(AtomicUsize::new(0));
            let worker_latch = latch.clone();
            let worker_barrier = barrier.clone();
            let worker = std::thread::spawn(move || {
                worker_barrier.wait();
                worker_latch.arm(ShutdownReason::Restart);
            });
            barrier.wait();
            let observed = calls.clone();
            let _subscription = latch
                .subscribe(move || {
                    observed.fetch_add(1, Ordering::SeqCst);
                })
                .unwrap();
            worker.join().unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(latch.state.lock().reason, Some(ShutdownReason::Restart));
        }
    }

    #[test]
    fn sentinel_command_shutdown_unregistration_and_dropped_weak_owner_are_bounded() {
        let latch = ShutdownLatch::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let subscription = latch
            .subscribe(move || {
                observed.fetch_add(1, Ordering::SeqCst);
            })
            .unwrap();
        assert!(latch.subscribe(|| {}).is_err());
        drop(subscription);
        let weak = Arc::downgrade(&calls);
        let _subscription = latch
            .subscribe(move || {
                if let Some(owner) = weak.upgrade() {
                    owner.fetch_add(1, Ordering::SeqCst);
                }
            })
            .unwrap();
        assert_eq!(Arc::strong_count(&calls), 1);
        drop(calls);
        latch.arm(ShutdownReason::UserExit);
        assert!(latch.state.lock().callback.is_none());
    }
}
