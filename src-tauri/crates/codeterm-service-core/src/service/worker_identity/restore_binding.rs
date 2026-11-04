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

pub const LINEAGE_PROOF_KIND: &str = "prior_session_binding_nonce_suffix";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SameSessionLineageProof {
    digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptBindingRestoreResult {
    Restored(AttemptBindingRestorationReceipt),
    AlreadyCurrent,
}

pub struct AttemptBindingRestoreTarget<'a> {
    pub plan_id: &'a str,
    pub task_id: &'a str,
    pub attempt_id: &'a str,
    pub owner_epoch: u64,
}

pub fn restore_attempt_binding_for_caller(
    service: &ServiceContext,
    target: AttemptBindingRestoreTarget<'_>,
    caller: &AuthenticatedPlanCaller,
    now_ms: u64,
) -> Result<AttemptBindingRestoreResult, PlanError> {
    let tab_id = caller
        .tab_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| Refusal::CallerTabMissing.refusal())?;
    let (tab, live_team) = {
        let mux = service.mux.read();
        let tab = mux
            .get_tab(tab_id)
            .cloned()
            .ok_or_else(|| Refusal::CallerTabNotFound.refusal())?;
        let live_team = tab
            .team_id
            .as_deref()
            .is_some_and(|team| mux.get_team(team).is_some());
        (tab, live_team)
    };
    let request =
        prepare_restoration(&service.plan_store, &tab, live_team, target, caller, now_ms)?;
    let result = service
        .plan_store
        .restore_current_attempt_binding(request.clone(), now_ms)?;
    let current = {
        let mux = service.mux.read();
        mux.get_tab(tab_id).is_some_and(|current| {
            same_restoration_authority(&tab, current)
                && current
                    .team_id
                    .as_deref()
                    .is_some_and(|team| mux.get_team(team).is_some())
        })
    };
    if !current {
        if matches!(result, RestoreCurrentAttemptBindingResult::Restored(_)) {
            service
                .plan_store
                .undo_restored_attempt_binding(&request, now_ms)?;
        }
        return Err(PlanError::Io(
            "attempt restoration conflict: live authority changed during SQL; retry by intent"
                .into(),
        ));
    }
    Ok(map_restore_result(result))
}

#[cfg(test)]
pub fn restore_attempt_binding_for_mux(
    store: &crate::service::plan::PlanStore,
    mux: &crate::multiplexer::state::MultiplexerState,
    target: AttemptBindingRestoreTarget<'_>,
    caller: &AuthenticatedPlanCaller,
    now_ms: u64,
) -> Result<AttemptBindingRestoreResult, PlanError> {
    let tab_id = caller
        .tab_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| Refusal::CallerTabMissing.refusal())?;
    let tab = mux
        .get_tab(tab_id)
        .ok_or_else(|| Refusal::CallerTabNotFound.refusal())?;
    let live_team = tab
        .team_id
        .as_deref()
        .is_some_and(|team| mux.get_team(team).is_some());
    let request = prepare_restoration(store, tab, live_team, target, caller, now_ms)?;
    store
        .restore_current_attempt_binding(request, now_ms)
        .map(map_restore_result)
}

fn prepare_restoration(
    store: &crate::service::plan::PlanStore,
    tab: &Tab,
    live_team: bool,
    target: AttemptBindingRestoreTarget<'_>,
    caller: &AuthenticatedPlanCaller,
    now_ms: u64,
) -> Result<RestoreCurrentAttemptBinding, PlanError> {
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
    let detail = store.task_binding_restore_projection(target.task_id, target.attempt_id)?;
    if detail.plan_id != target.plan_id || detail.task_id != target.task_id {
        return Err(Refusal::TargetMismatch.refusal());
    }
    let team_id = tab
        .team_id
        .as_deref()
        .filter(|team_id| !team_id.trim().is_empty())
        .ok_or_else(|| Refusal::TeamMissing.refusal())?;
    if !live_team {
        return Err(Refusal::TeamMissing.refusal());
    }
    if detail.team_id.as_deref() != Some(team_id) {
        return Err(Refusal::TeamMismatch.refusal());
    }
    if detail.attempt_count != 1 {
        return Err(Refusal::AttemptNotUnique.refusal());
    }
    let old_nonce = detail
        .attempt_nonce
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
    Ok(RestoreCurrentAttemptBinding {
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
    })
}

fn map_restore_result(result: RestoreCurrentAttemptBindingResult) -> AttemptBindingRestoreResult {
    match result {
        RestoreCurrentAttemptBindingResult::Restored(receipt) => {
            AttemptBindingRestoreResult::Restored(receipt)
        }
        RestoreCurrentAttemptBindingResult::AlreadyCurrent => {
            AttemptBindingRestoreResult::AlreadyCurrent
        }
    }
}

fn same_restoration_authority(a: &Tab, b: &Tab) -> bool {
    a.agent_type == b.agent_type
        && a.agent_session_id == b.agent_session_id
        && a.session_binding_nonce == b.session_binding_nonce
        && a.reported_session_binding_nonce == b.reported_session_binding_nonce
        && a.prior_session_binding_nonces == b.prior_session_binding_nonces
        && a.role == b.role
        && a.role_profile == b.role_profile
        && a.team_id == b.team_id
        && a.pty_id == b.pty_id
        && a.backing == b.backing
        && a.chat_view == b.chat_view
        && a.exit_suspected_at == b.exit_suspected_at
        && crate::service::plan_reconcile::tab_has_live_agent(b)
}

pub fn is_current_worker_role(tab: &Tab) -> bool {
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

pub fn prove_same_session_restoration(
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

pub fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", sha2::Sha256::digest(bytes))
}

#[cfg(test)]
mod tests;
