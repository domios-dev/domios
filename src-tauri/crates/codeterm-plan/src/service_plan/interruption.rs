use super::{PlanError, PlanStore};
use crate::plan::{ExecutionIntent, InterruptionCheckpoint, ProviderAllowance, TaskStatus};
use sha2::{Digest, Sha256};
mod adoption;
pub use adoption::CheckpointAdoption;

#[derive(Debug, Clone)]
pub struct TaskMutationAuthority {
    pub tab_id: String,
    pub session_id: String,
    pub session_binding_nonce: String,
    pub owner_epoch: u64,
}

pub(crate) fn next_owner_epoch(current: u64) -> Result<u64, PlanError> {
    if current >= i64::MAX as u64 {
        return Err(PlanError::Io("task owner epoch exhausted".into()));
    }
    Ok(current + 1)
}

pub(crate) fn authorize_task_mutation_for_owner(
    owner_tab: Option<&str>,
    task: &crate::plan::Task,
    authority: &TaskMutationAuthority,
) -> Result<(), PlanError> {
    // A retired owner leaves nobody to authenticate against and its superseded
    // epoch fences nothing; plan-level authority upstream already admitted this caller.
    if task.interruption.owner_tab_id.is_none() && task.interruption.owner_session_id.is_none() {
        return Ok(());
    }
    if task.interruption.owner_epoch != authority.owner_epoch {
        return Err(PlanError::Io("stale task mutation owner epoch".into()));
    }
    let orchestrator = owner_tab == Some(&authority.tab_id);
    let worker = !task.interruption.handoff_required
        && task.interruption.owner_tab_id.as_deref() == Some(&authority.tab_id);
    if orchestrator || worker {
        return Ok(());
    }
    // A refusal names both compared values because the stored ids are not exposed by another command.
    let same_tab = task.interruption.owner_tab_id.as_deref() == Some(&authority.tab_id);
    Err(PlanError::Io(format!(
        "stale task mutation authority: caller tab {} session {}; task owner tab {} session {}{}",
        authority.tab_id,
        authority.session_id,
        task.interruption.owner_tab_id.as_deref().unwrap_or("-"),
        task.interruption.owner_session_id.as_deref().unwrap_or("-"),
        if same_tab {
            " (same tab, session moved)"
        } else {
            ""
        },
    )))
}

#[cfg(test)]
mod tests;

pub fn checkpoint_digest(diff_artifact: &str) -> String {
    format!("{:x}", Sha256::digest(diff_artifact.as_bytes()))
}

/// Status label a provider-exhausted worker carries while paused. Distinct from
/// the destructive `handoff_required` path so the surface reads "will resume at
/// reset" and re-dispatch/recovery can tell a pause from an orphaned handoff.
pub const PROVIDER_EXHAUSTED_LABEL: &str = "provider_exhausted";

/// Receipt warning for mutating work on a low or exhausted provider allowance.
/// The allowance is advisory: work always proceeds.
pub fn provider_allowance_warning(
    intent: &ExecutionIntent,
    allowance: Option<&ProviderAllowance>,
    provider: &str,
) -> Option<String> {
    let allowance =
        allowance.filter(|allowance| *intent == ExecutionIntent::Mutating && allowance.is_low())?;
    let state = if allowance.remaining_pct <= 0.0 {
        "exhausted"
    } else {
        "below its reserve"
    };
    let warning = format!(
        "provider_allowance_low: provider {provider} allowance is {state} (remaining {:.1}%, \
         reserve {:.1}%); the work proceeds and may consume paid credits",
        allowance.remaining_pct, allowance.reserve_pct
    );
    log::warn!("[provider-allowance] {warning}");
    Some(warning)
}

impl PlanStore {
    #[allow(clippy::too_many_arguments)]
    #[cfg(any(test, feature = "test-support"))]
    pub fn assign_task_with_role_authorized(
        &self,
        plan_id: &str,
        task_id: &str,
        tab: &str,
        peer: Option<&str>,
        role: Option<&str>,
        authority: &TaskMutationAuthority,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.assign_task_with_optional_binding(
            plan_id,
            task_id,
            tab,
            peer,
            role,
            None,
            Some(authority),
            now_ms,
        )
    }

    fn attempt_is_active_for_interruption(
        task_status: &TaskStatus,
        attempt: &crate::plan::TaskExecutionAttempt,
    ) -> bool {
        attempt.reported_at.is_none()
            || (*task_status == TaskStatus::InProgress
                && attempt.continuation.epoch > 0
                && attempt.continuation.state.as_deref() == Some("continued")
                && attempt.continuation.delivered_at.is_some())
    }

    pub fn record_dispatch_policy(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        intent: ExecutionIntent,
        allowance: Option<ProviderAllowance>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_task_rows(
            plan_id,
            task_id,
            super::store_db::hot_write::TaskHistory::Attempt(attempt_id.into()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                let attempt = task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| attempt.id == attempt_id && attempt.closed_at.is_none())
                    .ok_or(PlanError::TaskNotFound)?;
                task.interruption.execution_intent = intent;
                task.interruption.allowance = allowance;
                task.interruption.owner_epoch = task.interruption.owner_epoch.max(1);
                task.interruption.owner_tab_id = Some(attempt.executor_tab_id.clone());
                task.interruption.owner_session_id = attempt.session_id.clone();
                attempt.interruption.owner_epoch = task.interruption.owner_epoch;
                task.updated_at = now_ms;
                Ok(())
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_task_interruption(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        owner_tab_id: &str,
        owner_session_id: &str,
        owner_epoch: u64,
        reason: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_task_rows(
            plan_id,
            task_id,
            super::store_db::hot_write::TaskHistory::Attempt(attempt_id.into()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                if task.interruption.owner_epoch != owner_epoch
                    || task.interruption.owner_tab_id.as_deref() != Some(owner_tab_id)
                    || task.interruption.owner_session_id.as_deref() != Some(owner_session_id)
                {
                    return Err(PlanError::Io("stale interruption owner".into()));
                }
                let task_status = task.status.clone();
                let attempt = task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| attempt.id == attempt_id)
                    .filter(|attempt| {
                        attempt.closed_at.is_none()
                            && Self::attempt_is_active_for_interruption(&task_status, attempt)
                            && attempt.executor_tab_id == owner_tab_id
                            && attempt.session_id.as_deref() == Some(owner_session_id)
                            && attempt.interruption.owner_epoch == owner_epoch
                    })
                    .ok_or_else(|| {
                        PlanError::Io("interruption attempt authority changed".into())
                    })?;
                attempt.closed_at = Some(now_ms);
                attempt.interruption.interrupted_at = Some(now_ms);
                attempt.interruption.reason = Some(reason.to_string());
                task.interruption.handoff_required = true;
                task.interruption.checkpoint = None;
                task.status_label = Some("checkpoint_sealing".into());
                task.assigned_tab_id = None;
                task.lane.tab_id = None;
                task.updated_at = now_ms;
                Ok(())
            },
        )
    }

    /// Non-destructive quiesce for a provider-exhausted worker. Unlike
    /// `begin_task_interruption`, the attempt stays OPEN and the owner
    /// (tab/session/epoch) is preserved, so the SAME live worker resumes at
    /// reset — the tab is never destroyed and the task is never epoch-locked to a
    /// dead owner. The task parks in `Paused`/`PROVIDER_EXHAUSTED_LABEL`.
    #[allow(clippy::too_many_arguments)]
    pub fn begin_provider_exhaustion_pause(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        owner_tab_id: &str,
        owner_session_id: &str,
        owner_epoch: u64,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_task_rows(
            plan_id,
            task_id,
            super::store_db::hot_write::TaskHistory::Attempt(attempt_id.into()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                if task.interruption.owner_epoch != owner_epoch
                    || task.interruption.owner_tab_id.as_deref() != Some(owner_tab_id)
                    || task.interruption.owner_session_id.as_deref() != Some(owner_session_id)
                {
                    return Err(PlanError::Io("stale interruption owner".into()));
                }
                let task_status = task.status.clone();
                let attempt = task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| attempt.id == attempt_id)
                    .filter(|attempt| {
                        attempt.closed_at.is_none()
                            && Self::attempt_is_active_for_interruption(&task_status, attempt)
                            && attempt.executor_tab_id == owner_tab_id
                            && attempt.session_id.as_deref() == Some(owner_session_id)
                            && attempt.interruption.owner_epoch == owner_epoch
                    })
                    .ok_or_else(|| {
                        PlanError::Io("interruption attempt authority changed".into())
                    })?;
                // Preserve the attempt and owner: this is a pause, not a handoff.
                attempt.interruption.reason = Some("provider_exhausted".into());
                task.interruption.handoff_required = false;
                task.status = TaskStatus::Paused;
                task.status_label = Some(PROVIDER_EXHAUSTED_LABEL.into());
                task.updated_at = now_ms;
                Ok(())
            },
        )
    }

    /// Attach the resumable checkpoint sealed after a pause began. Best-effort
    /// safety snapshot; the live worker resumes without it, but it lets recovery
    /// reconstruct the work if the tab dies while paused.
    pub fn finish_provider_exhaustion_pause(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        owner_epoch: u64,
        checkpoint: InterruptionCheckpoint,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        if checkpoint.diff_sha256 != checkpoint_digest(&checkpoint.diff_artifact)
            || checkpoint.task_id != task_id
            || checkpoint.attempt_id != attempt_id
            || checkpoint.owner_epoch != owner_epoch
        {
            return Err(PlanError::Io(
                "interruption checkpoint verification failed".into(),
            ));
        }
        self.with_task_rows(
            plan_id,
            task_id,
            super::store_db::hot_write::TaskHistory::Attempt(attempt_id.into()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                if task.status_label.as_deref() != Some(PROVIDER_EXHAUSTED_LABEL)
                    || task.interruption.owner_epoch != owner_epoch
                {
                    return Err(PlanError::Io(
                        "provider-exhaustion pause ownership changed".into(),
                    ));
                }
                let attempt = task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| {
                        attempt.id == attempt_id
                            && attempt.closed_at.is_none()
                            && attempt.interruption.owner_epoch == owner_epoch
                    })
                    .ok_or_else(|| PlanError::Io("paused attempt changed".into()))?;
                attempt.interruption.checkpoint_digest = Some(checkpoint.diff_sha256.clone());
                task.interruption.checkpoint = Some(checkpoint);
                task.updated_at = now_ms;
                Ok(())
            },
        )
    }

    /// Resume a paused provider-exhausted worker when quota returns. Same owner,
    /// same epoch — no adoption, no epoch bump. Clears the pause surface and the
    /// safety checkpoint so the live worker owns its own state again.
    pub fn resume_from_provider_exhaustion(
        &self,
        plan_id: &str,
        task_id: &str,
        owner_tab_id: &str,
        owner_session_id: &str,
        owner_epoch: u64,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_task_rows(
            plan_id,
            task_id,
            super::store_db::hot_write::TaskHistory::Core,
            now_ms,
            |edit| {
                let task = &mut edit.task;
                if task.status_label.as_deref() != Some(PROVIDER_EXHAUSTED_LABEL) {
                    return Err(PlanError::Io(
                        "task is not provider-exhaustion paused".into(),
                    ));
                }
                if task.interruption.owner_epoch != owner_epoch
                    || task.interruption.owner_tab_id.as_deref() != Some(owner_tab_id)
                    || task.interruption.owner_session_id.as_deref() != Some(owner_session_id)
                {
                    return Err(PlanError::Io("stale resume owner".into()));
                }
                task.interruption.checkpoint = None;
                task.status = TaskStatus::InProgress;
                task.status_label = None;
                task.updated_at = now_ms;
                Ok(())
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn finish_task_interruption(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        owner_epoch: u64,
        checkpoint: InterruptionCheckpoint,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        if checkpoint.diff_sha256 != checkpoint_digest(&checkpoint.diff_artifact)
            || checkpoint.task_id != task_id
            || checkpoint.attempt_id != attempt_id
            || checkpoint.owner_epoch != owner_epoch
        {
            return Err(PlanError::Io(
                "interruption checkpoint verification failed".into(),
            ));
        }
        self.with_task_rows(
            plan_id,
            task_id,
            super::store_db::hot_write::TaskHistory::Attempt(attempt_id.into()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                if !task.interruption.handoff_required
                    || task.interruption.owner_epoch != owner_epoch
                    || task.status_label.as_deref() != Some("checkpoint_sealing")
                {
                    return Err(PlanError::Io(
                        "interruption checkpoint ownership changed".into(),
                    ));
                }
                let attempt = task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| {
                        attempt.id == attempt_id
                            && attempt.closed_at.is_some()
                            && attempt.interruption.interrupted_at.is_some()
                            && attempt.interruption.owner_epoch == owner_epoch
                    })
                    .ok_or_else(|| PlanError::Io("interrupted attempt changed".into()))?;
                attempt.interruption.checkpoint_digest = Some(checkpoint.diff_sha256.clone());
                task.interruption.checkpoint = Some(checkpoint);
                task.status_label = Some("handoff_required".into());
                task.updated_at = now_ms;
                Ok(())
            },
        )
    }

    pub fn mark_interruption_checkpoint_failed(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        owner_epoch: u64,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_task_rows(
            plan_id,
            task_id,
            super::store_db::hot_write::TaskHistory::Attempt(attempt_id.into()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                let exact = task.execution_attempts.iter().any(|attempt| {
                    attempt.id == attempt_id
                        && attempt.interruption.owner_epoch == owner_epoch
                        && attempt.interruption.interrupted_at.is_some()
                });
                if !exact || !task.interruption.handoff_required {
                    return Err(PlanError::Io(
                        "interruption failure authority changed".into(),
                    ));
                }
                task.status_label = Some("handoff_checkpoint_failed".into());
                task.updated_at = now_ms;
                Ok(())
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn interrupt_task_attempt(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        owner_tab_id: &str,
        owner_session_id: &str,
        owner_epoch: u64,
        reason: &str,
        checkpoint: InterruptionCheckpoint,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.begin_task_interruption(
            plan_id,
            task_id,
            attempt_id,
            owner_tab_id,
            owner_session_id,
            owner_epoch,
            reason,
            now_ms,
        )?;
        self.finish_task_interruption(
            plan_id,
            task_id,
            attempt_id,
            owner_epoch,
            checkpoint,
            now_ms,
        )
    }

    pub fn verify_task_owner_epoch(
        &self,
        task_id: &str,
        tab_id: &str,
        _session_id: &str,
        owner_epoch: u64,
    ) -> Result<(), PlanError> {
        let owner = self.task_owner_epoch_projection(task_id)?;
        if !owner.matches_owner_epoch(tab_id, owner_epoch) {
            return Err(PlanError::Io("stale task owner epoch".into()));
        }
        Ok(())
    }
}
