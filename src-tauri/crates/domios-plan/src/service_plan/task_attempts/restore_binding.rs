use super::super::{AttemptBindingRefusalKind as Refusal, PlanError, PlanStore};
use crate::plan::{AttemptBindingRestorationReceipt, PlanStatus, TaskStatus, WorkerPlacement};

#[derive(Debug, Clone)]
pub struct RestoreCurrentAttemptBinding {
    pub plan_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub executor_tab_id: String,
    pub session_id: String,
    pub provider: String,
    pub team_id: String,
    pub owner_epoch: u64,
    pub expected_old_nonce: String,
    pub current_nonce: String,
    pub receipt: AttemptBindingRestorationReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreCurrentAttemptBindingResult {
    Restored(AttemptBindingRestorationReceipt),
    AlreadyCurrent,
}

impl PlanStore {
    pub fn restore_current_attempt_binding(
        &self,
        request: RestoreCurrentAttemptBinding,
        now_ms: u64,
    ) -> Result<RestoreCurrentAttemptBindingResult, PlanError> {
        let plan_id = request.plan_id.clone();
        self.with_plan_if_changed(&plan_id, now_ms, |plan| {
            if plan.status != PlanStatus::Active
                || request.owner_epoch == 0
                || request.plan_id.trim().is_empty()
                || request.task_id.trim().is_empty()
                || request.attempt_id.trim().is_empty()
                || request.executor_tab_id.trim().is_empty()
                || request.session_id.trim().is_empty()
                || request.provider.trim().is_empty()
                || request.expected_old_nonce.trim().is_empty()
                || request.current_nonce.trim().is_empty()
                || request.team_id.trim().is_empty()
                || plan.team_id.as_deref() != Some(request.team_id.as_str())
            {
                return Err(Refusal::PlanOrRequestInvalid.refusal());
            }
            let mut matches = plan
                .tasks
                .iter_mut()
                .filter(|task| task.id == request.task_id);
            let task = matches.next().ok_or(PlanError::TaskNotFound)?;
            if matches.next().is_some()
                || task.assigned_tab_id.as_deref() != Some(request.executor_tab_id.as_str())
                || task.lane.tab_id.as_deref() != Some(request.executor_tab_id.as_str())
                || task.peer.is_some()
                || task.competition.is_some()
                || (task.status != TaskStatus::InProgress && task.status != TaskStatus::InReview)
                || task.interruption.handoff_required
                || task.interruption.checkpoint.is_some()
                || task.interruption.owner_tab_id.as_deref()
                    != Some(request.executor_tab_id.as_str())
                || task.interruption.owner_session_id.as_deref()
                    != Some(request.session_id.as_str())
            {
                return Err(Refusal::TaskOwnershipMismatch.refusal());
            }
            if task.interruption.owner_epoch != request.owner_epoch {
                return Err(Refusal::StaleTaskEpoch.refusal());
            }
            let open_attempts = task
                .execution_attempts
                .iter()
                .filter(|attempt| attempt.closed_at.is_none() && attempt.retired_at.is_none())
                .collect::<Vec<_>>();
            if open_attempts.len() != 1
                || open_attempts[0].id != request.attempt_id
                || task
                    .execution_attempts
                    .iter()
                    .filter(|attempt| attempt.id == request.attempt_id)
                    .count()
                    != 1
            {
                return Err(Refusal::AttemptNotUnique.refusal());
            }
            let attempt = &open_attempts[0];
            if attempt.interruption.owner_epoch != request.owner_epoch {
                return Err(Refusal::StaleAttemptEpoch.refusal());
            }
            if attempt.executor_tab_id != request.executor_tab_id
                || attempt.provider.as_deref() != Some(request.provider.as_str())
                || attempt.agent_type.as_deref() != Some(request.provider.as_str())
                || attempt.session_id.as_deref() != Some(request.session_id.as_str())
                || !attempt
                    .role
                    .as_deref()
                    .is_some_and(|role| role.eq_ignore_ascii_case("worker"))
                || attempt.acknowledged_at.is_none()
                || attempt.interruption.interrupted_at.is_some()
                || attempt.interruption.reason.is_some()
                || attempt.interruption.checkpoint_digest.is_some()
                || !continuation_allows_binding_restore(task, attempt)
                || attempt.competition_group_id.is_some()
                || attempt.competition_lane.is_some()
                || attempt.mesh_peer.is_some()
                || attempt.remote_dispatch_state.is_some()
                || attempt.placement != WorkerPlacement::IsolatedLane
            {
                return Err(Refusal::AttemptAuthorityInvalid.refusal());
            }
            if attempt.session_binding_nonce.as_deref() == Some(request.current_nonce.as_str()) {
                return Ok(None);
            }
            if attempt.session_binding_nonce.as_deref() != Some(request.expected_old_nonce.as_str())
                || request.expected_old_nonce == request.current_nonce
                || !valid_receipt(&request, now_ms)
                || attempt
                    .binding_restorations
                    .iter()
                    .any(|receipt| receipt.operation_id == request.receipt.operation_id)
            {
                return Err(Refusal::ReceiptInvalid.refusal());
            }
            let attempt = task
                .execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == request.attempt_id)
                .expect("validated unique open attempt remains present");
            attempt.session_binding_nonce = Some(request.current_nonce.clone());
            attempt.binding_restorations.push(request.receipt.clone());
            Ok(Some(RestoreCurrentAttemptBindingResult::Restored(
                request.receipt,
            )))
        })
        .map(|result| result.unwrap_or(RestoreCurrentAttemptBindingResult::AlreadyCurrent))
    }
}

fn continuation_allows_binding_restore(
    task: &crate::plan::Task,
    attempt: &crate::plan::TaskExecutionAttempt,
) -> bool {
    let mut idle = attempt.continuation.clone();
    idle.epoch = 0;
    idle == Default::default()
        && (attempt.continuation.epoch == 0
            || (task.status == TaskStatus::InReview
                && task.review_round > 0
                && attempt.continuation.epoch >= 2
                && attempt
                    .report
                    .as_ref()
                    .is_some_and(|report| report.status.as_deref() == Some("in_review"))
                && task.review_evidence.iter().any(|evidence| {
                    evidence.verdict == crate::plan::ReviewVerdict::Bounce
                        && attempt
                            .reported_at
                            .is_some_and(|reported_at| evidence.at < reported_at)
                })))
}

fn valid_receipt(request: &RestoreCurrentAttemptBinding, now_ms: u64) -> bool {
    let receipt = &request.receipt;
    receipt.operation_id.trim() != ""
        && receipt.restored_at == now_ms
        && receipt.attempt_id == request.attempt_id
        && receipt.executor_tab_id == request.executor_tab_id
        && receipt.session_id == request.session_id
        && receipt.provider == request.provider
        && receipt.owner_epoch == request.owner_epoch
        && receipt.old_nonce_digest == nonce_digest(&request.expected_old_nonce)
        && receipt.current_nonce_digest == nonce_digest(&request.current_nonce)
        && receipt.lineage_proof_kind == "prior_session_binding_nonce_suffix"
        && is_sha256_digest(&receipt.lineage_proof_digest)
}

fn is_sha256_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn nonce_digest(nonce: &str) -> String {
    use sha2::Digest;
    format!("sha256:{:x}", sha2::Sha256::digest(nonce.as_bytes()))
}

#[cfg(test)]
mod tests;
