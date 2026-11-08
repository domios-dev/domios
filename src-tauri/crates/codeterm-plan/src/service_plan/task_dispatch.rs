mod checkout_binding;
mod lifecycle;
mod reservation;
mod rollback;
mod unblock;
mod undo;
mod worker_ack_subject;

use super::lane_metadata::normalize_lane_metadata;
use super::{PlanError, PlanStore};

#[derive(Debug, Clone)]
pub(super) struct DispatchedAttemptMetadata {
    launch_profile: Option<String>,
    model: Option<String>,
    requested_model: Option<String>,
    provider_resolved_model: Option<String>,
    reasoning_effort: Option<String>,
}

pub(super) use reservation::DispatchReservationOwner;
pub use reservation::TaskDispatchReservation;
pub use rollback::DispatchRollback;
pub(super) use unblock::{task_respawn_status, task_startup_status};
pub use undo::{DispatchStart, DispatchUndo};
pub use worker_ack_subject::{
    ambiguous_subject_cause, superseded_subject_cause, unresolved_subject_cause, LiveTabAttempt,
    LiveTaskAttemptRead, ACK_CAUSE_AMBIGUOUS_ATTEMPTS, ACK_CAUSE_ATTEMPT_SUPERSEDED,
    ACK_CAUSE_NO_LIVE_ATTEMPT, ACK_CAUSE_PLAN_NOT_ACTIVE, ACK_CAUSE_TASK_NOT_OPEN,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchStartupOwner {
    pub attempt_id: String,
    pub tab_id: String,
}

pub(super) fn startup_attempt<'a>(
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
    use super::task_authority_projection::startup::{startup_subject_current, StartupSubject};
    let latest_unclosed = task
        .execution_attempts
        .iter()
        .rfind(|attempt| attempt.closed_at.is_none())
        .map(|attempt| attempt.id.as_str());
    let latest_group = task
        .execution_attempts
        .iter()
        .rfind(|attempt| attempt.closed_at.is_none() && attempt.competition_group_id.is_some())
        .and_then(|attempt| attempt.competition_group_id.as_deref());
    startup_subject_current(
        tab_id,
        task.assigned_tab_id.as_deref(),
        StartupSubject {
            id: &attempt.id,
            executor: &attempt.executor_tab_id,
            session: attempt.session_id.as_deref(),
            group: attempt.competition_group_id.as_deref(),
        },
        latest_unclosed,
        latest_group,
    )
    .then_some((attempt, attempt.competition_group_id.is_some()))
}

/// A validated startup attempt is already proven to be the task's live executor,
/// so an in-progress task is that owner respawning, never a foreign claim.
fn startup_status_allows(status: &crate::plan::TaskStatus) -> bool {
    matches!(
        status,
        crate::plan::TaskStatus::Pending | crate::plan::TaskStatus::InProgress
    )
}

pub(super) fn current_dispatch_startup_owner(
    task: &crate::plan::Task,
) -> Option<DispatchStartupOwner> {
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
        || !startup_status_allows(&task.status)
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
        let status = self.database.task_status_projection(plan_id, task_id)?;
        task_startup_status(&status).map(|_| ())
    }

    pub fn dispatch_startup_owner_candidate(
        &self,
        plan_id: &str,
        task_id: &str,
    ) -> Result<Option<DispatchStartupOwner>, PlanError> {
        self.database
            .dispatch_startup_owner_projection(plan_id, task_id)
    }

    pub fn worker_startup_attempt_owns(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
        session_id: Option<&str>,
    ) -> Result<bool, PlanError> {
        match self
            .database
            .worker_startup_state_projection(plan_id, task_id, tab_id, attempt_id)
        {
            Ok((read, _)) => Ok(read.owns(tab_id, session_id, true)),
            Err(PlanError::NotFound | PlanError::TaskNotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn worker_ack_deadline_attempt_projection(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
    ) -> Result<Option<String>, PlanError> {
        match self
            .database
            .worker_startup_state_projection(plan_id, task_id, tab_id, attempt_id)
        {
            Ok((read, active)) => {
                Ok((active && read.owns(tab_id, None, true)).then(|| attempt_id.to_string()))
            }
            Err(PlanError::NotFound | PlanError::TaskNotFound) => Ok(None),
            Err(error) => Err(error),
        }
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
        let (current, _) = self
            .database
            .worker_startup_state_projection(plan_id, task_id, tab_id, attempt_id)?;
        let owns_startup = current.owns(tab_id, None, false);
        if !owns_startup {
            return Ok(false);
        }
        self.with_task_rows(
            plan_id,
            task_id,
            super::store_db::hot_write::TaskHistory::LiveAndTarget(attempt_id.into()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                let authority = startup_attempt(task, tab_id, attempt_id);
                let owns_startup = authority.is_some_and(|(attempt, _)| {
                    startup_status_allows(&task.status) && attempt.acknowledged_at.is_none()
                });
                if !owns_startup {
                    return Ok(false);
                }
                let competitive = task.execution_attempts.iter().any(|attempt| {
                    attempt.id == attempt_id && attempt.competition_group_id.is_some()
                });
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
            },
        )
        .or_else(|error| match error {
            PlanError::TaskNotFound => Ok(false),
            error => Err(error),
        })
    }

    pub fn mark_worker_startup_cleanup_failed(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        self.with_task_rows(
            plan_id,
            task_id,
            super::store_db::hot_write::TaskHistory::Attempts,
            now_ms,
            |edit| {
                let task = &mut edit.task;
                if task.id != task_id {
                    return Ok(false);
                }
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
            },
        )
        .or_else(|error| match error {
            PlanError::TaskNotFound => Ok(false),
            error => Err(error),
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
        .map(|started| started.attempt_id)
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
        .map(|started| started.attempt_id)
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
        .map(|started| started.attempt_id)
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
        requested_model: Option<String>,
        provider_resolved_model: Option<String>,
        reasoning_effort: Option<String>,
        launch_profile: Option<String>,
        observed_startup_owner: Option<DispatchStartupOwner>,
        observed_startup_owner_is_live: bool,
        startup_owner_observation_made: bool,
        now_ms: u64,
    ) -> Result<DispatchStart, PlanError> {
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
                launch_profile,
                model,
                requested_model,
                provider_resolved_model,
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
        .map(|started| started.attempt_id)
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
    ) -> Result<DispatchStart, PlanError> {
        let dispatch_attempt_id = reserved_attempt_id
            .map(str::to_string)
            .unwrap_or_else(|| crate::plan::ids::short_id("attempt"));
        let mut attempt_id = None;
        let mut undo = None;
        let shared_binding: Option<super::shared_checkout::SharedDispatchInspection> =
            if placement == crate::plan::WorkerPlacement::Shared {
                Some(self.inspect_shared_dispatch(plan_id, task_id, now_ms)?)
            } else {
                None
            };
        let writer_turn = self.mutation_writer.enter();
        if let Some(inspected) = shared_binding.as_ref() {
            let task = self.database.shared_dispatch_projection(plan_id, task_id)?;
            inspected.recheck(&task.checkout, &task.files)?;
            super::shared_checkout::validate_shared_ownership_at(
                &self.checkout_peer_facts(&task.checkout)?,
                plan_id,
                &task.task_id,
                &task.checkout,
                &task.files,
            )
            .map_err(PlanError::InvalidPlanText)?;
        } else if placement == crate::plan::WorkerPlacement::CanonicalWriter {
            let checkout = self.database.dispatch_checkout_projection(plan_id)?;
            super::shared_checkout::validate_exclusive_checkout_at(
                &self.checkout_peer_facts(&checkout)?,
                plan_id,
                task_id,
                &checkout,
            )
            .map_err(PlanError::InvalidPlanText)?;
        }
        let write = self.with_task_rows_in_writer(
            &writer_turn,
            plan_id,
            task_id,
            super::store_db::hot_write::TaskHistory::LiveAndReported,
            now_ms,
            |edit| {
                let dispatch_status = task_startup_status(&edit.task.status)?;
                undo = Some(DispatchUndo::capture(
                    plan_id,
                    &edit.mode,
                    &edit.task,
                    &dispatch_attempt_id,
                    None,
                ));
                edit.unblock("dispatch", &dispatch_attempt_id, now_ms);
                lane.tab_id = Some(tab_id.to_string());
                edit.progression = Some((dispatch_status, true));
                let task = &mut edit.task;
                let current_owner = current_dispatch_startup_owner(task);
                let dispatch_in_flight = startup_owner_observation_made
                    && current_owner.as_ref().is_some_and(|owner| {
                        observed_startup_owner.as_ref() != Some(owner)
                            || observed_startup_owner_is_live
                    });
                if dispatch_in_flight {
                    let owner = current_owner.expect("in-flight owner must be present");
                    return Err(PlanError::DispatchInFlight {
                        attempt_id: owner.attempt_id,
                        tab_id: owner.tab_id,
                    });
                }
                lane.destination = task.lane.destination.clone().or(lane.destination);
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
                    attempt.launch_profile = metadata.launch_profile.clone();
                    attempt.model = metadata.model.clone();
                    attempt.requested_model = metadata.requested_model.clone();
                    attempt.provider_resolved_model = metadata.provider_resolved_model.clone();
                    attempt.reasoning_effort = metadata.reasoning_effort.clone();
                }
                attempt.authorized_gates = authorized_gates.clone();
                attempt.placement = placement;
                attempt.execution_intent = Some(intent.clone());
                if let Some(inspected) = shared_binding.as_ref() {
                    attempt.checkout_path = Some(inspected.checkout.clone());
                    attempt.shared_start_sha = Some(inspected.head.clone());
                }
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
                edit.mode = crate::plan::PlanMode::Execution;
                undo.as_mut()
                    .expect("undo captured before dispatch mutation")
                    .finish(&edit.mode, task);
                Ok(())
            },
        );
        write?;
        let attempt_id = attempt_id.ok_or_else(|| {
            Self::task_readback_error("task_dispatch_start", "attempt was not created")
        })?;
        Ok(DispatchStart {
            attempt_id,
            undo: undo.expect("undo captured before successful dispatch"),
        })
    }
}
