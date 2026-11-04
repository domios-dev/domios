use crate::multiplexer::commands::MuxState;
use crate::service::plan::{PlanError, PlanStore};

use super::{ServiceState, SpawnIdentityProvenance};

pub mod restore_binding;
pub mod review_bounce;

pub use review_bounce::restore_review_attempt_binding_for_mux;

pub use restore_binding::{
    restore_attempt_binding_for_caller, restore_attempt_binding_for_mux,
    AttemptBindingRestoreResult, AttemptBindingRestoreTarget,
};

pub fn respawn_title_provenance(explicit_title: bool) -> SpawnIdentityProvenance {
    if explicit_title {
        SpawnIdentityProvenance::UserSupplied
    } else {
        SpawnIdentityProvenance::Suggested
    }
}

pub fn persist_attempt_executor_identity(
    store: &PlanStore,
    svc: &ServiceState,
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    title: &str,
    now: u64,
) -> Result<(), PlanError> {
    let result = persist_attempt_executor_identity_for_mux(
        store, &svc.mux, plan_id, task_id, tab_id, title, now,
    );
    if result.is_ok() {
        svc.persist_current_attempt_session_model(tab_id);
    }
    result
}

pub fn persist_attempt_executor_identity_for_mux(
    store: &PlanStore,
    mux: &MuxState,
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    title: &str,
    now: u64,
) -> Result<(), PlanError> {
    let mux = mux.read();
    let binding = mux
        .get_tab(tab_id)
        .and_then(super::task_assignment::authenticated_tab_binding);
    store.update_attempt_executor_identity(
        plan_id,
        task_id,
        tab_id,
        Some(title),
        binding.as_ref().map(|binding| binding.session_id.as_str()),
        binding
            .as_ref()
            .map(|binding| binding.session_binding_nonce.as_str()),
        now,
    )
}

#[cfg(test)]
#[path = "worker_identity/tests.rs"]
mod tests;
