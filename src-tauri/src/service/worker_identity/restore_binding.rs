use crate::multiplexer::types::{AgentRole, PriorBindingNonce, Tab};
use crate::plan::AttemptBindingRestorationReceipt;
use crate::service::plan::task_attempts::{
    RestoreCurrentAttemptBinding, RestoreCurrentAttemptBindingResult,
};
use crate::service::plan::{AttemptBindingRefusalKind as Refusal, PlanError};
use crate::service::{
    task_assignment::authenticated_tab_binding, AuthenticatedPlanCaller, ServiceContext,
};
use sha2::Digest;

const LINEAGE_PROOF_KIND: &str = "prior_session_binding_nonce_suffix";

#[derive(Debug, Clone, PartialEq, Eq)]
struct SameSessionLineageProof {
    digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AttemptBindingRestoreResult {
    Restored(AttemptBindingRestorationReceipt),
    AlreadyCurrent,
}

pub(crate) struct AttemptBindingRestoreTarget<'a> {
    pub plan_id: &'a str,
    pub task_id: &'a str,
    pub attempt_id: &'a str,
    pub owner_epoch: u64,
}

pub(crate) fn restore_attempt_binding_for_caller(
    service: &ServiceContext,
    target: AttemptBindingRestoreTarget<'_>,
    caller: &AuthenticatedPlanCaller,
    now_ms: u64,
) -> Result<AttemptBindingRestoreResult, PlanError> {
    let mux = service.mux.read();
    restore_attempt_binding_for_mux(&service.plan_store, &mux, target, caller, now_ms)
}

pub(crate) fn restore_attempt_binding_for_mux(
    store: &crate::service::plan::PlanStore,
    mux: &crate::multiplexer::state::MultiplexerState,
    target: AttemptBindingRestoreTarget<'_>,
    caller: &AuthenticatedPlanCaller,
    now_ms: u64,
) -> Result<AttemptBindingRestoreResult, PlanError> {
    let tab_id = caller
        .tab_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Refusal::CallerTabMissing.refusal())?;
    let current_nonce = caller
        .binding_nonce
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Refusal::CallerNonceMissing.refusal())?;
    let tab = mux
        .get_tab(tab_id)
        .ok_or_else(|| Refusal::CallerTabNotFound.refusal())?;
    if !is_current_worker_role(tab) {
        return Err(Refusal::WorkerRoleRequired.refusal());
    }
    if tab.session_binding_nonce.as_deref() != Some(current_nonce)
        || tab.reported_session_binding_nonce.as_deref() != Some(current_nonce)
    {
        return Err(Refusal::CurrentBindingRequired.refusal());
    }
    if !crate::service::plan_reconcile::tab_has_live_agent(tab) {
        return Err(Refusal::LiveAgentRequired.refusal());
    }
    let binding =
        authenticated_tab_binding(tab).ok_or_else(|| Refusal::CurrentBindingRequired.refusal())?;
    let session_id = binding.session_id.as_str();
    if caller
        .session_id
        .as_deref()
        .is_some_and(|claim| claim != session_id)
    {
        return Err(Refusal::CallerSessionMismatch.refusal());
    }
    let provider = tab
        .agent_type
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Refusal::ProviderMissing.refusal())?;
    let detail = store
        .show_task(target.task_id)
        .ok_or(PlanError::TaskNotFound)?;
    if detail.plan_id != target.plan_id || detail.task.id != target.task_id {
        return Err(Refusal::TargetMismatch.refusal());
    }
    let team_id = tab
        .team_id
        .as_deref()
        .filter(|team_id| !team_id.trim().is_empty())
        .ok_or_else(|| Refusal::TeamMissing.refusal())?;
    if mux.get_team(team_id).is_none() {
        return Err(Refusal::TeamMissing.refusal());
    }
    let plan = store.get(target.plan_id).ok_or(PlanError::NotFound)?;
    if plan.team_id.as_deref() != Some(team_id) {
        return Err(Refusal::TeamMismatch.refusal());
    }
    let attempts = detail
        .task
        .execution_attempts
        .iter()
        .filter(|attempt| attempt.id == target.attempt_id)
        .collect::<Vec<_>>();
    let [attempt] = attempts.as_slice() else {
        return Err(Refusal::AttemptNotUnique.refusal());
    };
    let old_nonce = attempt
        .session_binding_nonce
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Refusal::AttemptNonceMissing.refusal())?;
    let proof = if old_nonce == current_nonce {
        SameSessionLineageProof {
            digest: digest(current_nonce.as_bytes()),
        }
    } else {
        prove_same_session_restoration(
            &tab.prior_session_binding_nonces,
            old_nonce,
            current_nonce,
            session_id,
        )?
    };
    let receipt = AttemptBindingRestorationReceipt {
        operation_id: uuid::Uuid::new_v4().to_string(),
        restored_at: now_ms,
        attempt_id: target.attempt_id.into(),
        executor_tab_id: tab_id.into(),
        session_id: session_id.into(),
        provider: provider.into(),
        owner_epoch: target.owner_epoch,
        old_nonce_digest: digest(old_nonce.as_bytes()),
        current_nonce_digest: digest(current_nonce.as_bytes()),
        lineage_proof_kind: LINEAGE_PROOF_KIND.into(),
        lineage_proof_digest: proof.digest,
    };
    let result = store.restore_current_attempt_binding(
        RestoreCurrentAttemptBinding {
            plan_id: target.plan_id.into(),
            task_id: target.task_id.into(),
            attempt_id: target.attempt_id.into(),
            executor_tab_id: tab_id.into(),
            session_id: session_id.into(),
            provider: provider.into(),
            team_id: team_id.into(),
            owner_epoch: target.owner_epoch,
            expected_old_nonce: old_nonce.into(),
            current_nonce: current_nonce.into(),
            receipt,
        },
        now_ms,
    )?;
    match result {
        RestoreCurrentAttemptBindingResult::Restored(receipt) => {
            Ok(AttemptBindingRestoreResult::Restored(receipt))
        }
        RestoreCurrentAttemptBindingResult::AlreadyCurrent => {
            Ok(AttemptBindingRestoreResult::AlreadyCurrent)
        }
    }
}

fn is_current_worker_role(tab: &Tab) -> bool {
    if tab.role != Some(AgentRole::Worker) {
        return false;
    }
    let Some(kind_id) = tab.role_kind_id() else {
        return false;
    };
    let profile_matches_kind = tab
        .role_profile
        .as_deref()
        .map(str::trim)
        .filter(|profile| !profile.is_empty())
        .is_none_or(|profile| profile == kind_id);
    profile_matches_kind
        && !codeterm_types::agent::role_kind::manages(tab.role, tab.role_profile.as_deref())
        && !codeterm_types::agent::role_kind::anchors_team(tab.role, tab.role_profile.as_deref())
        && !codeterm_types::agent::role_kind::has_team_authority(
            tab.role,
            tab.role_profile.as_deref(),
        )
}

fn prove_same_session_restoration(
    lineage: &[PriorBindingNonce],
    old_nonce: &str,
    current_nonce: &str,
    session_id: &str,
) -> Result<SameSessionLineageProof, PlanError> {
    if old_nonce.is_empty() || current_nonce.is_empty() || session_id.is_empty() {
        return Err(Refusal::LineageInputInvalid.refusal());
    }
    if lineage.iter().any(|entry| entry.nonce == current_nonce) {
        return Err(Refusal::LineageContainsCurrentNonce.refusal());
    }
    let old_indices = lineage
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| (entry.nonce == old_nonce).then_some(index))
        .collect::<Vec<_>>();
    let [old_index] = old_indices.as_slice() else {
        return Err(Refusal::LineageOldNonceNotUnique.refusal());
    };
    let suffix = &lineage[*old_index..];
    let Some(first) = suffix.first() else {
        return Err(Refusal::LineageOldNonceNotUnique.refusal());
    };
    if first.session_id.as_deref() != Some(session_id)
        || first
            .resume_target_session_id
            .as_deref()
            .is_some_and(|target| target != session_id)
    {
        return Err(Refusal::LineageSessionMismatch.refusal());
    }
    let mut seen = std::collections::HashSet::new();
    for entry in suffix {
        if entry.nonce.is_empty() || !seen.insert(entry.nonce.as_str()) {
            return Err(Refusal::LineageNonceInvalid.refusal());
        }
        if entry.session_id.as_deref() == Some(session_id)
            && entry
                .resume_target_session_id
                .as_deref()
                .is_none_or(|target| target == session_id)
        {
            continue;
        }
        if entry.session_id.is_none()
            && entry.resume_target_session_id.as_deref() == Some(session_id)
        {
            continue;
        }
        return Err(Refusal::LineageSuffixUnqualified.refusal());
    }
    let bytes = serde_json::to_vec(suffix).map_err(|_| Refusal::LineageEncodingFailed.refusal())?;
    Ok(SameSessionLineageProof {
        digest: digest(&bytes),
    })
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", sha2::Sha256::digest(bytes))
}

#[cfg(test)]
mod tests;
