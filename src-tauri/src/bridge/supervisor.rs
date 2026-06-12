use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use tauri::async_runtime::JoinHandle;
use tokio::sync::Notify;

/// Capability passed to messaging-service implementations so they can
/// report poll outcomes without owning the supervisor's internal state.
pub trait BridgeStatusSink: Send + Sync {
    fn mark_poll_ok(&self);
    fn mark_poll_error(&self, err: &str);
}

#[derive(Debug, Clone)]
pub struct BridgeRuntimeState {
    pub service: String,
    pub active: bool,
    pub started_at: Option<DateTime<Utc>>,
    pub last_poll_ok: Option<DateTime<Utc>>,
    pub last_poll_error: Option<String>,
    pub error_count: u32,
}

impl BridgeRuntimeState {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            active: false,
            started_at: None,
            last_poll_ok: None,
            last_poll_error: None,
            error_count: 0,
        }
    }
}

pub struct BridgeSupervisor {
    state: Arc<Mutex<BridgeRuntimeState>>,
    cancel: Arc<Notify>,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
    op_lock: Arc<tokio::sync::Mutex<()>>,
}

impl Default for BridgeSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl BridgeSupervisor {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(BridgeRuntimeState::new("telegram"))),
            cancel: Arc::new(Notify::new()),
            task: Arc::new(Mutex::new(None)),
            op_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub async fn op_guard(&self) -> tokio::sync::OwnedMutexGuard<()> {
        self.op_lock.clone().lock_owned().await
    }

    pub fn as_status_sink(self: &Arc<Self>) -> Arc<dyn BridgeStatusSink> {
        self.clone()
    }

    pub fn cancel_token(&self) -> Arc<Notify> {
        self.cancel.clone()
    }

    pub fn snapshot(&self) -> BridgeRuntimeState {
        self.state.lock().clone()
    }

    pub fn is_active(&self) -> bool {
        self.state.lock().active
    }

    pub fn set_task(&self, handle: JoinHandle<()>, started_at: DateTime<Utc>) {
        if let Some(prev) = self.task.lock().replace(handle) {
            prev.abort();
        }
        let mut s = self.state.lock();
        s.active = true;
        s.started_at = Some(started_at);
        s.last_poll_ok = None;
        s.last_poll_error = None;
        s.error_count = 0;
    }

    pub async fn stop(&self, timeout: Duration) -> Result<Duration, String> {
        let handle = self.task.lock().take();
        {
            let mut s = self.state.lock();
            s.active = false;
        }

        let Some(mut handle) = handle else {
            return Ok(Duration::ZERO);
        };

        let cancel = self.cancel.clone();
        let nudger = tokio::spawn(async move {
            loop {
                cancel.notify_waiters();
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        });

        let started = Instant::now();
        let outcome = tokio::time::timeout(timeout, &mut handle).await;
        nudger.abort();
        let _ = nudger.await;
        match outcome {
            Ok(_join_result) => Ok(started.elapsed()),
            Err(_) => {
                handle.abort();
                let _ = handle.await;
                Err(format!(
                    "bridge stop timed out after {}ms",
                    timeout.as_millis()
                ))
            }
        }
    }
}

impl BridgeStatusSink for BridgeSupervisor {
    fn mark_poll_ok(&self) {
        let mut s = self.state.lock();
        s.last_poll_ok = Some(Utc::now());
        s.last_poll_error = None;
        s.error_count = 0;
    }

    fn mark_poll_error(&self, err: &str) {
        let mut s = self.state.lock();
        s.last_poll_error = Some(err.to_string());
        s.error_count = s.error_count.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn supervisor_stop_idempotent_when_not_running() {
        let sup = BridgeSupervisor::new();
        let elapsed = sup.stop(Duration::from_secs(1)).await.expect("stop ok");
        assert_eq!(elapsed, Duration::ZERO);
        assert!(!sup.is_active());
    }

    #[tokio::test]
    async fn supervisor_stop_signals_cancel() {
        let sup = BridgeSupervisor::new();
        let cancel = sup.cancel_token();
        let observed = Arc::new(AtomicBool::new(false));
        let observed_in = observed.clone();

        let handle = tauri::async_runtime::spawn(async move {
            cancel.notified().await;
            observed_in.store(true, Ordering::SeqCst);
        });
        sup.set_task(handle, Utc::now());
        assert!(sup.is_active());

        let elapsed = sup
            .stop(Duration::from_secs(1))
            .await
            .expect("stop should succeed");
        assert!(elapsed < Duration::from_secs(1));
        assert!(observed.load(Ordering::SeqCst));
        assert!(!sup.is_active());
    }

    #[tokio::test]
    async fn supervisor_stop_timeout_aborts_task() {
        let sup = BridgeSupervisor::new();

        let canary = Arc::new(());
        let canary_in = canary.clone();
        let handle = tauri::async_runtime::spawn(async move {
            let _hold = canary_in;
            std::future::pending::<()>().await;
        });
        sup.set_task(handle, Utc::now());
        assert_eq!(Arc::strong_count(&canary), 2);

        let res = sup.stop(Duration::from_millis(150)).await;
        assert!(res.is_err(), "should timeout");
        assert!(!sup.is_active());

        tokio::task::yield_now().await;
        assert_eq!(
            Arc::strong_count(&canary),
            1,
            "stop must abort + join the handle so task locals drop — leaked task would keep holding the Telegram lock"
        );
    }

    #[tokio::test]
    async fn nudger_does_not_kill_the_next_poller() {
        // Regression guard for the iteration-3 nudger-after-stop race: a stale notify_waiters() from stop()'s nudger must not wake a freshly-set task
        // that registers cancel.notified() afterwards.
        let sup = BridgeSupervisor::new();
        let cancel = sup.cancel_token();

        let h1 = tauri::async_runtime::spawn(async move {
            cancel.notified().await;
        });
        sup.set_task(h1, Utc::now());
        sup.stop(Duration::from_secs(1)).await.expect("clean stop");

        let cancel = sup.cancel_token();
        let killed = Arc::new(AtomicBool::new(false));
        let killed_in = killed.clone();
        let h2 = tauri::async_runtime::spawn(async move {
            cancel.notified().await;
            killed_in.store(true, Ordering::SeqCst);
        });
        sup.set_task(h2, Utc::now());

        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            !killed.load(Ordering::SeqCst),
            "fresh poller must not be woken by stale notify from prior stop's nudger"
        );

        sup.stop(Duration::from_secs(1)).await.expect("second stop");
    }

    #[test]
    fn supervisor_state_snapshot_round_trip() {
        let sup = BridgeSupervisor::new();
        sup.mark_poll_error("boom");
        sup.mark_poll_error("boom");
        let snap = sup.snapshot();
        assert_eq!(snap.service, "telegram");
        assert!(!snap.active);
        assert_eq!(snap.error_count, 2);
        assert_eq!(snap.last_poll_error.as_deref(), Some("boom"));

        sup.mark_poll_ok();
        let snap = sup.snapshot();
        assert_eq!(snap.error_count, 0);
        assert!(snap.last_poll_ok.is_some());
        assert!(snap.last_poll_error.is_none());
    }
}
