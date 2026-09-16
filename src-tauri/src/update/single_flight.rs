//! Process-wide single flight for update application.
//!
//! Every route that can launch an installer goes through a lease. The lease is
//! unforgeable outside this module, so an installer cannot start without one.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};

use super::apply::{ApplyOptions, ApplyResult};
use super::UpdateState;
#[cfg(test)]
use super::UpdatePhase;

/// Wire discriminants. A refused second request must never read as "nothing to
/// install", so `ALREADY_IN_FLIGHT` is distinct from `NO_UPDATE`.
pub const ALREADY_IN_FLIGHT: &str = "already_in_flight";
pub const NO_UPDATE: &str = "no_update";
pub const UPDATING: &str = "updating";

pub struct UpdateGate {
    in_flight: AtomicBool,
}

impl UpdateGate {
    pub const fn new() -> Self {
        Self {
            in_flight: AtomicBool::new(false),
        }
    }

    /// Grants at most one lease at a time. A caller that loses is refused, not
    /// queued: a second installer must never start behind the first.
    pub fn try_acquire(&self) -> Option<UpdateLease<'_>> {
        // Constructed only on the winning exchange: a lease built eagerly and
        // discarded would release the winner's claim on drop.
        if self
            .in_flight
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            Some(UpdateLease { gate: self })
        } else {
            None
        }
    }

    pub fn is_in_flight(&self) -> bool {
        self.in_flight.load(Ordering::SeqCst)
    }

    fn release(&self) {
        self.in_flight.store(false, Ordering::SeqCst);
    }
}

/// Releases on drop — success, error and panic alike. A lease that leaked on
/// the failure path would block every later update.
pub struct UpdateLease<'a> {
    gate: &'a UpdateGate,
}

impl Drop for UpdateLease<'_> {
    fn drop(&mut self) {
        self.gate.release();
    }
}

static GLOBAL_GATE: UpdateGate = UpdateGate::new();

pub fn global() -> &'static UpdateGate {
    &GLOBAL_GATE
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateRequestOutcome {
    Applied(ApplyResult),
    AlreadyInFlight,
}

impl UpdateRequestOutcome {
    pub fn discriminant(&self) -> &'static str {
        match self {
            Self::Applied(ApplyResult::NoUpdate) => NO_UPDATE,
            Self::Applied(ApplyResult::Restarting) => UPDATING,
            Self::AlreadyInFlight => ALREADY_IN_FLIGHT,
        }
    }
}

/// Runs `operation` while owning a lease the caller acquired earlier: the HTTP
/// route acquires before it answers, then hands the lease to the applying
/// thread.
pub(crate) async fn run_holding_lease<F, Fut, T>(
    lease: UpdateLease<'_>,
    operation: F,
) -> Result<T, String>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, String>>,
{
    let _lease = lease;
    operation().await
}

/// Acquire-run-release in one step, for a caller that awaits the update inline.
pub(crate) async fn run_single_flight<F, Fut>(
    gate: &UpdateGate,
    operation: F,
) -> Result<UpdateRequestOutcome, String>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<ApplyResult, String>>,
{
    let Some(lease) = gate.try_acquire() else {
        return Ok(UpdateRequestOutcome::AlreadyInFlight);
    };
    run_holding_lease(lease, operation)
        .await
        .map(UpdateRequestOutcome::Applied)
}

pub async fn apply_with_lease(
    lease: UpdateLease<'_>,
    app: &tauri::AppHandle,
    opts: ApplyOptions,
    update_state: std::sync::Arc<parking_lot::Mutex<UpdateState>>,
) -> Result<ApplyResult, String> {
    run_holding_lease(lease, || {
        super::apply::check_and_apply(app, opts, update_state)
    })
    .await
}

pub async fn install_staged_deb_with_lease(
    lease: UpdateLease<'_>,
    app: &tauri::AppHandle,
    version: String,
    path: std::path::PathBuf,
    initiating_tab_id: Option<&str>,
) -> Result<(), String> {
    run_holding_lease(lease, || {
        super::apply::install_staged_deb_and_restart(app, version, path, initiating_tab_id)
    })
    .await
}

pub async fn request_apply(
    gate: &UpdateGate,
    app: &tauri::AppHandle,
    opts: ApplyOptions,
    update_state: std::sync::Arc<parking_lot::Mutex<UpdateState>>,
) -> Result<UpdateRequestOutcome, String> {
    run_single_flight(gate, || {
        super::apply::check_and_apply(app, opts, update_state)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    #[derive(Default)]
    struct FakeApplier {
        applies: AtomicUsize,
        installers: AtomicUsize,
    }

    impl FakeApplier {
        fn apply(&self, result: ApplyResult) -> Result<ApplyResult, String> {
            self.applies.fetch_add(1, Ordering::SeqCst);
            if result == ApplyResult::Restarting {
                self.installers.fetch_add(1, Ordering::SeqCst);
            }
            Ok(result)
        }

        fn applies(&self) -> usize {
            self.applies.load(Ordering::SeqCst)
        }

        fn installers(&self) -> usize {
            self.installers.load(Ordering::SeqCst)
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn update_is_single_flight_across_callers() {
        let gate: &'static UpdateGate = Box::leak(Box::new(UpdateGate::new()));
        let fake = Arc::new(FakeApplier::default());
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel::<UpdatePhase>();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();

        // Caller A — the HTTP route shape: acquire, hand the lease to the worker.
        let lease = gate.try_acquire().expect("the first request acquires");
        let applier = fake.clone();
        let worker = tokio::spawn(async move {
            run_holding_lease(lease, || async move {
                let state = std::sync::Arc::new(parking_lot::Mutex::new(UpdateState {
                    install_method: super::super::InstallMethod::Unknown,
                    last_check: None,
                    last_check_time: None,
                    channel: super::super::Channel::Stable,
                    phase: UpdatePhase::Idle,
                    last_error: Some("stale error".to_string()),
                    phase_changed_at: None,
                    downloaded_bytes: 0,
                    download_total_bytes: None,
                }));
                super::super::apply::begin_apply(&state);
                entered_tx
                    .send(state.lock().phase)
                    .expect("test receiver alive");
                release_rx.await.map_err(|e| e.to_string())?;
                applier.apply(ApplyResult::Restarting)
            })
            .await
        });

        // A declared entry, not a delay: A owns the gate from here.
        assert_eq!(
            entered_rx.await.expect("the first apply entered"),
            UpdatePhase::Checking
        );

        // Caller B — the Tauri command shape, while A is still in flight.
        let second = run_single_flight(gate, || {
            let applier = fake.clone();
            async move { applier.apply(ApplyResult::Restarting) }
        })
        .await
        .expect("a refusal is an outcome, not an error");

        assert_eq!(second, UpdateRequestOutcome::AlreadyInFlight);
        assert_eq!(second.discriminant(), ALREADY_IN_FLIGHT);
        assert_ne!(
            second.discriminant(),
            UpdateRequestOutcome::Applied(ApplyResult::NoUpdate).discriminant(),
            "a refusal must be distinguishable from no update available"
        );

        release_tx.send(()).expect("the worker is alive");
        let first = worker
            .await
            .expect("worker joined")
            .expect("the first apply succeeded");
        assert_eq!(first, ApplyResult::Restarting);

        assert_eq!(fake.applies(), 1, "check_and_apply ran exactly once");
        assert_eq!(fake.installers(), 1, "exactly one installer launch");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn update_guard_releases_on_success_and_failure() {
        let gate = UpdateGate::new();
        let fake = FakeApplier::default();

        let first = run_single_flight(&gate, || async { fake.apply(ApplyResult::Restarting) })
            .await
            .expect("a lone request is never refused");
        assert_eq!(
            first,
            UpdateRequestOutcome::Applied(ApplyResult::Restarting),
            "a single update request still proceeds"
        );
        assert!(!gate.is_in_flight(), "the success path released the guard");

        let failed = run_single_flight(&gate, || async {
            Err::<ApplyResult, String>("installer launch failed".to_string())
        })
        .await;
        assert_eq!(failed.unwrap_err(), "installer launch failed");
        assert!(
            !gate.is_in_flight(),
            "a failed update must never block the next one"
        );

        let after_failure =
            run_single_flight(&gate, || async { fake.apply(ApplyResult::NoUpdate) })
                .await
                .expect("the request after a failure is accepted");
        assert_eq!(
            after_failure,
            UpdateRequestOutcome::Applied(ApplyResult::NoUpdate)
        );
        assert_eq!(after_failure.discriminant(), NO_UPDATE);
        assert_eq!(fake.applies(), 2);
        assert_eq!(fake.installers(), 1);
    }

    #[test]
    fn the_gate_is_one_process_wide_instance() {
        assert!(std::ptr::eq(global(), global()));
    }
}
