use crate::multiplexer::commands::MuxState;
use crate::service::plan::{PlanError, PlanStore};

use super::{ServiceState, SpawnIdentityProvenance};

pub mod restore_binding;
pub mod review_bounce;

pub use review_bounce::restore_review_attempt_binding;
#[cfg(test)]
pub use review_bounce::restore_review_attempt_binding_for_mux;

pub use restore_binding::{
    restore_attempt_binding_for_caller, AttemptBindingRestoreResult, AttemptBindingRestoreTarget,
};

#[cfg(test)]
pub use restore_binding::restore_attempt_binding_for_mux;

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
    let capture = || {
        let mux = mux.read();
        let tab = mux.get_tab(tab_id);
        let binding = tab
            .and_then(super::task_assignment::authenticated_tab_binding)
            .map(|binding| (binding.session_id, binding.session_binding_nonce));
        let profile = tab
            .and_then(|tab| tab.agent_runtime_config.as_ref())
            .and_then(|config| config.get("launchProfile"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        (binding, profile)
    };
    let binding = capture();
    let receipt = store.update_attempt_executor_identity_with_undo(
        plan_id,
        task_id,
        tab_id,
        Some(title),
        binding.0.as_ref().map(|binding| binding.0.as_str()),
        binding.0.as_ref().map(|binding| binding.1.as_str()),
        binding.1.as_deref(),
        now,
    )?;
    if capture() != binding {
        store.undo_attempt_executor_identity(receipt, now)?;
        return Err(PlanError::Io(
            "executor identity conflict: live session changed during SQL; retry by intent".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "worker_identity/tests.rs"]
mod tests;
