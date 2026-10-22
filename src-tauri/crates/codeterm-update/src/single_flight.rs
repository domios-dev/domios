//! Process-wide single flight for update application.
//!
//! Every route that can launch an installer goes through a lease. The lease is
//! unforgeable outside this module, so an installer cannot start without one.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};

use super::apply::{ApplyOptions, ApplyResult};
#[cfg(test)]
use super::UpdatePhase;
#[cfg(test)]
use super::UpdateState;

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
    app: &dyn crate::application_host::ApplicationHost,
    opts: ApplyOptions,
) -> Result<ApplyResult, String> {
    run_holding_lease(lease, || app.check_and_apply(opts)).await
}

pub async fn request_apply(
    gate: &UpdateGate,
    app: &dyn crate::application_host::ApplicationHost,
    opts: ApplyOptions,
) -> Result<UpdateRequestOutcome, String> {
    run_single_flight(gate, || app.check_and_apply(opts)).await
}

#[cfg(test)]
#[path = "single_flight/tests.rs"]
mod tests;
