mod lifecycle;
mod reservation;
mod rollback;
mod unblock;
mod worker_ack_subject;

use super::lane_metadata::normalize_lane_metadata;
use super::readback::pending_after_verified;
use super::{PlanError, PlanStore};

#[derive(Debug, Clone)]
pub(super) struct DispatchedAttemptMetadata {
    model: Option<String>,
    reasoning_effort: Option<String>,
}

pub(super) use reservation::DispatchReservationOwner;
pub use reservation::TaskDispatchReservation;
pub use rollback::DispatchRollback;
pub(super) use unblock::{
    record_blocked_task_unblock, restore_blocked_unblock, task_respawn_status,
    task_startup_status,
};
pub use worker_ack_subject::{
    ambiguous_subject_cause, superseded_subject_cause, unresolved_subject_cause, LiveTabAttempt,
    ACK_CAUSE_AMBIGUOUS_ATTEMPTS, ACK_CAUSE_ATTEMPT_SUPERSEDED, ACK_CAUSE_NO_LIVE_ATTEMPT,
    ACK_CAUSE_PLAN_NOT_ACTIVE, ACK_CAUSE_TASK_NOT_OPEN,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchStartupOwner {
    pub attempt_id: String,
    pub tab_id: String,
}

fn startup_attempt<'a>(
    task: &'a crate::plan::Task,
    tab_id: &str,
    attempt_id: &str,
) -> Option<(&'a crate::plan::TaskExecutionAttempt, bool)> {
    let attempt = task.execution_attempts.iter().find(|attempt| {
        attempt.id == attempt_id
            && attempt.executor_tab_id == tab_id
            && attempt.closed_at.is_none()
            && attempt.retired_at.is_none()
    })?;
    if let Some(group_id) = attempt.competition_group_id.as_deref() {
        if attempt
            .session_id
            .as_deref()
            .is_none_or(|session_id| session_id.trim().is_empty())
        {
            return None;
        }
        let current_group = task
            .execution_attempts
            .iter()
            .rev()
            .find(|attempt| attempt.closed_at.is_none() && attempt.competition_group_id.is_some())
            .and_then(|attempt| attempt.competition_group_id.as_deref());
        return (current_group == Some(group_id)).then_some((attempt, true));
    }
    let current = task
        .execution_attempts
        .iter()
        .rfind(|attempt| attempt.closed_at.is_none());
    (task.assigned_tab_id.as_deref() == Some(tab_id)
        && current.is_some_and(|current| current.id == attempt_id))
    .then_some((attempt, false))
}

/// Entering `in_progress` needs a persisted executor: a competitive task proves
/// it with the exact live attempt, an assigned task with its own task-level tab.
fn progression_authority(
    plan: &crate::plan::Plan,
    task_index: usize,
    competitive: bool,
    attempt_id: &str,
) -> Result<(), PlanError> {
    let task = &plan.tasks[task_index];
    if competitive {
        super::mutations::validate_competitive_attempt_status_transition(
            plan,
            &task.id,
            &task.status,
            &crate::plan::TaskStatus::InProgress,
            attempt_id,
        )
    } else {
        super::mutations::validate_task_status_transition(
            plan,
            &task.id,
            &task.status,
            &crate::plan::TaskStatus::InProgress,
            None,
        )
    }
}

/// A validated startup attempt is already proven to be the task's live executor,
/// so an in-progress task is that owner respawning, never a foreign claim.
fn startup_status_allows(task: &crate::plan::Task) -> bool {
    matches!(
        task.status,
        crate::plan::TaskStatus::Pending | crate::plan::TaskStatus::InProgress
    )
}

fn current_dispatch_startup_owner(task: &crate::plan::Task) -> Option<DispatchStartupOwner> {
    let tab_id = task.assigned_tab_id.as_deref()?;
    let attempt = task
        .execution_attempts
        .iter()
        .rfind(|attempt| attempt.closed_at.is_none() && attempt.retired_at.is_none())?;
    if attempt.executor_tab_id != tab_id
        || attempt.executor_title.is_none()
        || attempt.acknowledged_at.is_some()
        || attempt.reported_at.is_some()
        || attempt.report.is_some()
        || attempt.competition_group_id.is_some()
        || !startup_status_allows(task)
        || startup_attempt(task, tab_id, &attempt.id).is_none()
    {
        return None;
    }
    Some(DispatchStartupOwner {
        attempt_id: attempt.id.clone(),
        tab_id: tab_id.to_string(),
    })
}

impl PlanStore {
    pub fn validate_task_startup_status(
        &self,
        plan_id: &str,
        task_id: &str,
    ) -> Result<(), PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let task = crate::plan::task_key::find_task(&plan.tasks, task_id)
            .ok_or(PlanError::TaskNotFound)?;
        task_startup_status(&task.status).map(|_| ())
    }

    pub fn dispatch_startup_owner_candidate(
        &self,
        plan_id: &str,
        task_id: &str,
    ) -> Option<DispatchStartupOwner> {
        self.get(plan_id).and_then(|plan| {
            crate::plan::task_key::find_task(&plan.tasks, task_id)
                .and_then(current_dispatch_startup_owner)
        })
    }

    pub fn worker_startup_attempt_owns(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
        session_id: Option<&str>,
    ) -> bool {
        self.get(plan_id).is_some_and(|plan| {
            crate::plan::task_key::find_task(&plan.tasks, task_id)
                .and_then(|task| {
                    startup_attempt(task, tab_id, attempt_id).map(|(attempt, _)| (task, attempt))
                })
                .is_some_and(|(task, attempt)| {
                    startup_status_allows(task)
                        && attempt.acknowledged_at.is_none()
                        && attempt.reported_at.is_none()
                        && attempt.report.is_none()
                        && session_id.is_none_or(|session_id| {
                            attempt.session_id.as_deref() == Some(session_id)
                        })
                })
        })
    }

    pub fn mark_worker_startup_deadline(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
        final_deadline: bool,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        let current = self.read_task_for_verification(plan_id, task_id)?;
        let authority = startup_attempt(&current, tab_id, attempt_id);
        let owns_startup = authority.is_some_and(|(attempt, _)| {
            startup_status_allows(&current) && attempt.acknowledged_at.is_none()
        });
        if !owns_startup {
            return Ok(false);
        }
        self.with_plan(plan_id, now_ms, |plan| {
            let Some(task) = crate::plan::task_key::find_task_mut(&mut plan.tasks, task_id) else {
                return Ok(false);
            };
            let authority = startup_attempt(task, tab_id, attempt_id);
            let owns_startup = authority.is_some_and(|(attempt, _)| {
                startup_status_allows(task) && attempt.acknowledged_at.is_none()
            });
            if !owns_startup {
                return Ok(false);
            }
            let competitive = task
                .execution_attempts
                .iter()
                .any(|attempt| attempt.id == attempt_id && attempt.competition_group_id.is_some());
            if final_deadline {
                let attempt = task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| attempt.id == attempt_id)
                    .expect("validated startup attempt must remain present");
                attempt.closed_at = Some(now_ms);
                if !competitive {
                    super::mutations::clear_task_assignment(task);
                }
                let live_competitors = task.execution_attempts.iter().any(|attempt| {
                    attempt.closed_at.is_none() && attempt.competition_group_id.is_some()
                });
                if task.status == crate::plan::TaskStatus::InProgress && !live_competitors {
                    task.status = crate::plan::TaskStatus::Pending;
                    task.status_changed_at = now_ms;
                }
                task.status_label = Some(if live_competitors {
                    "competitive".into()
                } else {
                    "startup_failed".into()
                });
            } else {
                task.status_label = Some(if competitive {
                    "competitive".into()
                } else {
                    "startup_unbound".into()
                });
            }
            task.updated_at = now_ms;
            Ok(true)
        })
    }

    pub fn mark_worker_startup_cleanup_failed(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            let Some(task) = plan.tasks.iter_mut().find(|task| task.id == task_id) else {
                return Ok(false);
            };
            let target = task
                .execution_attempts
                .iter()
                .find(|attempt| attempt.id == attempt_id);
            let fenced_attempt_is_current = target.is_some_and(|attempt| {
                if attempt.closed_at.is_none() {
                    return false;
                }
                if let Some(group_id) = attempt.competition_group_id.as_deref() {
                    return task
                        .execution_attempts
                        .iter()
                        .rev()
                        .find_map(|candidate| candidate.competition_group_id.as_deref())
                        == Some(group_id);
                }
                task.assigned_tab_id.is_none()
                    && task
                        .execution_attempts
                        .last()
                        .is_some_and(|candidate| candidate.id == attempt_id)
            });
            if !fenced_attempt_is_current {
                return Ok(false);
            }
            task.status_label = Some("startup_cleanup_failed".into());
            task.updated_at = now_ms;
            Ok(true)
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_task_dispatch(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        executor_title: &str,
        lane: crate::plan::TaskLaneMetadata,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        self.begin_task_dispatch_with_policy(
            plan_id,
            task_id,
            tab_id,
            executor_title,
            lane,
            crate::plan::ExecutionIntent::Unspecified,
            None,
            None,
            now_ms,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_task_dispatch_with_policy(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        executor_title: &str,
        lane: crate::plan::TaskLaneMetadata,
        intent: crate::plan::ExecutionIntent,
        allowance: Option<crate::plan::ProviderAllowance>,
        provider: Option<&str>,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        self.begin_task_dispatch_with_policy_and_dispatched_metadata(
            plan_id,
            task_id,
            tab_id,
            executor_title,
            lane,
            intent,
            allowance,
            provider,
            None,
            None,
            crate::plan::WorkerPlacement::IsolatedLane,
            None,
            false,
            false,
            None,
            now_ms,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_task_dispatch_with_reserved_attempt(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        executor_title: &str,
        lane: crate::plan::TaskLaneMetadata,
        intent: crate::plan::ExecutionIntent,
        allowance: Option<crate::plan::ProviderAllowance>,
        provider: Option<&str>,
        attempt_id: &str,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        self.begin_task_dispatch_with_policy_and_dispatched_metadata(
            plan_id,
            task_id,
            tab_id,
            executor_title,
            lane,
            intent,
            allowance,
            provider,
            Some(attempt_id),
            None,
            crate::plan::WorkerPlacement::IsolatedLane,
            None,
            false,
            false,
            None,
            now_ms,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_task_dispatch_with_reserved_attempt_and_gates(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        executor_title: &str,
        lane: crate::plan::TaskLaneMetadata,
        intent: crate::plan::ExecutionIntent,
        allowance: Option<crate::plan::ProviderAllowance>,
        provider: Option<&str>,
        attempt_id: &str,
        authorized_gates: Vec<String>,
        placement: crate::plan::WorkerPlacement,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        self.begin_task_dispatch_with_policy_and_dispatched_metadata(
            plan_id,
            task_id,
            tab_id,
            executor_title,
            lane,
            intent,
            allowance,
            provider,
            Some(attempt_id),
            Some(authorized_gates),
            placement,
            None,
            false,
            false,
            None,
            now_ms,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_task_dispatch_with_reserved_attempt_and_gates_and_observation(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        executor_title: &str,
        lane: crate::plan::TaskLaneMetadata,
        intent: crate::plan::ExecutionIntent,
        allowance: Option<crate::plan::ProviderAllowance>,
        provider: Option<&str>,
        attempt_id: &str,
        authorized_gates: Option<Vec<String>>,
        placement: crate::plan::WorkerPlacement,
        model: Option<String>,
        reasoning_effort: Option<String>,
        observed_startup_owner: Option<DispatchStartupOwner>,
        observed_startup_owner_is_live: bool,
        startup_owner_observation_made: bool,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        self.begin_task_dispatch_with_policy_and_dispatched_metadata(
            plan_id,
            task_id,
            tab_id,
            executor_title,
            lane,
            intent,
            allowance,
            provider,
            Some(attempt_id),
            authorized_gates,
            placement,
            observed_startup_owner,
            observed_startup_owner_is_live,
            startup_owner_observation_made,
            Some(DispatchedAttemptMetadata {
                model,
                reasoning_effort,
            }),
            now_ms,
        )
    }

    pub fn begin_task_dispatch_with_observation(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        executor_title: &str,
        lane: crate::plan::TaskLaneMetadata,
        observed_startup_owner: Option<DispatchStartupOwner>,
        observed_startup_owner_is_live: bool,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        self.begin_task_dispatch_with_policy_and_dispatched_metadata(
            plan_id,
            task_id,
            tab_id,
            executor_title,
            lane,
            crate::plan::ExecutionIntent::Unspecified,
            None,
            None,
            None,
            None,
            crate::plan::WorkerPlacement::IsolatedLane,
            observed_startup_owner,
            observed_startup_owner_is_live,
            true,
            None,
            now_ms,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn begin_task_dispatch_with_policy_and_dispatched_metadata(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        executor_title: &str,
        mut lane: crate::plan::TaskLaneMetadata,
        intent: crate::plan::ExecutionIntent,
        allowance: Option<crate::plan::ProviderAllowance>,
        provider: Option<&str>,
        reserved_attempt_id: Option<&str>,
        authorized_gates: Option<Vec<String>>,
        placement: crate::plan::WorkerPlacement,
        observed_startup_owner: Option<DispatchStartupOwner>,
        observed_startup_owner_is_live: bool,
        startup_owner_observation_made: bool,
        dispatched_metadata: Option<DispatchedAttemptMetadata>,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        super::interruption::provider_allowance_preflight(&intent, allowance.as_ref(), now_ms)?;
        let dispatch_attempt_id = reserved_attempt_id
            .map(str::to_string)
            .unwrap_or_else(|| crate::plan::ids::short_id("attempt"));
        let mut attempt_id = None;
        let mut original_status = None;
        let write = self.with_plan(plan_id, now_ms, |plan| {
            let task_index = crate::plan::task_key::find_task_index(&plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let dispatch_status = task_startup_status(&plan.tasks[task_index].status)?;
            record_blocked_task_unblock(
                plan,
                task_index,
                "dispatch",
                Some(&dispatch_attempt_id),
                now_ms,
            );
            lane.tab_id = Some(tab_id.to_string());
            let lane_update = super::TaskLaneUpdate {
                tab_id: Some(Some(tab_id.to_string())),
                ..Default::default()
            };
            super::mutations::validate_task_status_transition_with_lane(
                plan,
                &plan.tasks[task_index].id,
                &dispatch_status,
                &crate::plan::TaskStatus::InProgress,
                None,
                Some(&lane_update),
            )?;
            original_status = Some(dispatch_status);
            let task = &mut plan.tasks[task_index];
            let current_owner = current_dispatch_startup_owner(task);
            let dispatch_in_flight = startup_owner_observation_made
                && current_owner.as_ref().is_some_and(|owner| {
                    observed_startup_owner.as_ref() != Some(owner) || observed_startup_owner_is_live
                });
            if dispatch_in_flight {
                let owner = current_owner.expect("in-flight owner must be present");
                return Err(PlanError::DispatchInFlight {
                    attempt_id: owner.attempt_id,
                    tab_id: owner.tab_id,
                });
            }
            lane.destination = task.lane.destination.clone();
            lane.cleanup_status = task.lane.cleanup_status.clone();
            lane.cleanup_hints = task.lane.cleanup_hints.clone();
            task.lane = normalize_lane_metadata(lane);
            task.assigned_tab_id = Some(tab_id.to_string());
            task.taken_by_role = Some("worker".into());
            task.peer = None;
            let mut attempt = super::task_attempts::new_assignment_attempt_with_id(
                task,
                tab_id,
                None,
                Some("worker"),
                Some(&dispatch_attempt_id),
                now_ms,
            );
            attempt.executor_title = Some(executor_title.to_string());
            attempt.provider = provider.map(str::to_string);
            attempt.agent_type = provider.map(str::to_string);
            if let Some(metadata) = dispatched_metadata.as_ref() {
                attempt.model = metadata.model.clone();
                attempt.reasoning_effort = metadata.reasoning_effort.clone();
            }
            attempt.authorized_gates = authorized_gates.clone();
            attempt.placement = placement;
            task.interruption.execution_intent = intent.clone();
            task.interruption.allowance = allowance.clone();
            task.interruption.owner_epoch = task.interruption.owner_epoch.max(1);
            task.interruption.owner_tab_id = Some(tab_id.to_string());
            task.interruption.owner_session_id = None;
            attempt.interruption.owner_epoch = task.interruption.owner_epoch;
            attempt_id = Some(attempt.id.clone());
            super::task_attempts::record_noncompetitive_replacement_attempt(
                &mut task.execution_attempts,
                attempt,
                now_ms,
            );
            task.status_label = Some("starting".into());
            task.updated_at = now_ms;
            plan.mode = crate::plan::PlanMode::Execution;
            Ok(())
        });
        let write = match write {
            Ok(()) => Ok(()),
            Err(error @ PlanError::WritePending { .. }) => Err(error),
            Err(error) => return Err(error),
        };
        let attempt_id = attempt_id.ok_or_else(|| {
            Self::task_readback_error("task_dispatch_start", "attempt was not created")
        })?;
        let original_status = original_status.ok_or_else(|| {
            Self::task_readback_error("task_dispatch_start", "original status was not captured")
        })?;
        let verified = (|| {
            let task = self.read_task_for_verification(plan_id, task_id)?;
            let attempt = task
                .execution_attempts
                .iter()
                .find(|attempt| attempt.id == attempt_id)
                .ok_or_else(|| {
                    Self::task_readback_error("task_dispatch_start", "attempt missing")
                })?;
            if task.status != original_status
                || task.status_label.as_deref() != Some("starting")
                || task.assigned_tab_id.as_deref() != Some(tab_id)
                || task.lane.tab_id.as_deref() != Some(tab_id)
                || attempt.executor_tab_id != tab_id
                || attempt.executor_title.as_deref() != Some(executor_title)
                || attempt.provider.as_deref() != provider
                || attempt.agent_type.as_deref() != provider
                || dispatched_metadata.as_ref().is_some_and(|metadata| {
                    attempt.model != metadata.model
                        || attempt.reasoning_effort != metadata.reasoning_effort
                })
                || attempt.session_id.is_some()
                || task.interruption.execution_intent != intent
                || task.interruption.allowance != allowance
                || task.interruption.owner_tab_id.as_deref() != Some(tab_id)
                || attempt.interruption.owner_epoch != task.interruption.owner_epoch
                || attempt.authorized_gates != authorized_gates
                || attempt.placement != placement
            {
                return Err(Self::task_readback_error(
                    "task_dispatch_start",
                    format!("starting dispatch state did not match for {}", task.id),
                ));
            }
            Ok(())
        })();
        pending_after_verified(write, verified, Some(task_id.to_string()))?;
        Ok(attempt_id)
    }

}
