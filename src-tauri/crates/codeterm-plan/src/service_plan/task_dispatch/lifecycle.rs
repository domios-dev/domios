use super::super::lane_metadata::normalize_lane_metadata;
use super::super::readback::pending_after_verified;
use super::super::{PlanError, PlanStore};
use super::{
    ambiguous_subject_cause, progression_authority, startup_attempt, startup_status_allows,
    superseded_subject_cause, unresolved_subject_cause, LiveTabAttempt,
};

impl PlanStore {
    pub fn finish_task_dispatch(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
        session_id: &str,
        session_binding_nonce: &str,
        mut lane: crate::plan::TaskLaneMetadata,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let write = self.with_plan(plan_id, now_ms, |plan| {
            let task_index = crate::plan::task_key::find_task_index(&plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let attempt_index = plan.tasks[task_index]
                .execution_attempts
                .iter()
                .position(|attempt| attempt.id == attempt_id)
                .ok_or(PlanError::TaskNotFound)?;
            if plan.tasks[task_index].execution_attempts[attempt_index]
                .acknowledged_at
                .is_some()
                && plan.tasks[task_index].status != crate::plan::TaskStatus::InProgress
            {
                super::super::mutations::validate_task_status_transition(
                    plan,
                    &plan.tasks[task_index].id,
                    &plan.tasks[task_index].status,
                    &crate::plan::TaskStatus::InProgress,
                    None,
                )?;
            }
            let canonical_checkout = if plan.canonical_project_path.trim().is_empty() {
                plan.project_path.clone()
            } else {
                plan.canonical_project_path.clone()
            };
            let task = &mut plan.tasks[task_index];
            if task.assigned_tab_id.as_deref() != Some(tab_id) {
                return Err(Self::task_readback_error(
                    "task_dispatch_finish",
                    "assigned tab changed before readiness",
                ));
            }
            lane.tab_id = Some(tab_id.to_string());
            // Destination and cleanup are integration-owned metadata. They may be
            // recorded after dispatch acceptance, so readiness must not roll them back.
            lane.destination = task.lane.destination.clone().or(lane.destination);
            lane.cleanup_status = task.lane.cleanup_status.clone();
            lane.cleanup_hints = task.lane.cleanup_hints.clone();
            task.lane = normalize_lane_metadata(lane);
            let attempt = &mut task.execution_attempts[attempt_index];
            if attempt.executor_tab_id != tab_id
                || attempt.closed_at.is_some()
                || attempt.retired_at.is_some()
            {
                return Err(Self::task_readback_error(
                    "task_dispatch_finish",
                    "starting attempt no longer owns the tab",
                ));
            }
            attempt.session_id = Some(session_id.to_string());
            attempt.session_binding_nonce = Some(session_binding_nonce.to_string());
            task.interruption.owner_session_id = Some(session_id.to_string());
            task.interruption.owner_tab_id = Some(tab_id.to_string());
            task.interruption.owner_epoch = task.interruption.owner_epoch.max(1);
            attempt.interruption.owner_epoch = task.interruption.owner_epoch;
            attempt.worktree_path = task.lane.worktree_path.clone();
            attempt.branch = task.lane.branch.clone();
            attempt.base = task.lane.base.clone();
            attempt.destination = task.lane.destination.clone();
            if attempt.placement == crate::plan::WorkerPlacement::CanonicalWriter {
                // No base keeps the report guard strict: HEAD must match and any dirt refuses.
                attempt.worktree_path = Some(canonical_checkout);
                attempt.base = None;
            }
            if attempt.acknowledged_at.is_some() {
                if task.status != crate::plan::TaskStatus::InProgress {
                    task.status_changed_at = now_ms;
                }
                task.status = crate::plan::TaskStatus::InProgress;
                task.status_label = Some("acknowledged".into());
            } else {
                task.status_label = Some("awaiting_worker_ack".into());
            }
            task.updated_at = now_ms;
            Ok(())
        });
        let verified = (|| {
            let task = self.read_task_for_verification(plan_id, task_id)?;
            let attempt = task
                .execution_attempts
                .iter()
                .find(|attempt| attempt.id == attempt_id)
                .ok_or_else(|| {
                    Self::task_readback_error("task_dispatch_finish", "attempt missing")
                })?;
            let ready_state = if attempt.acknowledged_at.is_some() {
                task.status == crate::plan::TaskStatus::InProgress
                    && task.status_label.as_deref() == Some("acknowledged")
            } else {
                task.status_label.as_deref() == Some("awaiting_worker_ack")
            };
            if !ready_state
                || task.assigned_tab_id.as_deref() != Some(tab_id)
                || attempt.session_id.as_deref() != Some(session_id)
                || attempt.session_binding_nonce.as_deref() != Some(session_binding_nonce)
            {
                return Err(Self::task_readback_error(
                    "task_dispatch_finish",
                    format!("ready dispatch state did not match for {}", task.id),
                ));
            }
            Ok(())
        })();
        pending_after_verified(write, verified, Some(task_id.to_string()))
    }

    pub fn acknowledge_task_attempt(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
        now_ms: u64,
    ) -> Result<super::super::task_attempts::TaskAttemptAcknowledgement, PlanError> {
        let _writer_turn = self.mutation_writer.enter();
        let current_task = self.read_task_for_verification(plan_id, task_id)?;
        let Some((current_attempt, competitive)) =
            startup_attempt(&current_task, tab_id, attempt_id)
        else {
            return Err(Self::task_readback_error(
                "task_dispatch_ack",
                "attempt no longer owns the acknowledging tab",
            ));
        };
        if current_attempt.acknowledged_at.is_some() {
            let attempt = current_attempt.clone();
            return Ok(super::super::task_attempts::TaskAttemptAcknowledgement {
                plan_id: plan_id.to_string(),
                task_id: current_task.id,
                attempt,
                already_acknowledged: true,
            });
        }
        if !startup_status_allows(&current_task) {
            return Err(Self::task_readback_error(
                "task_dispatch_ack",
                "only a live startup assignment can be acknowledged",
            ));
        }
        let mut acknowledged = None;
        let write = self.with_plan_in_writer(plan_id, now_ms, |plan| {
            let task_index = crate::plan::task_key::find_task_index(&plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let attempt_index = plan.tasks[task_index]
                .execution_attempts
                .iter()
                .position(|attempt| attempt.id == attempt_id)
                .ok_or(PlanError::TaskNotFound)?;
            let authority = startup_attempt(&plan.tasks[task_index], tab_id, attempt_id);
            let competitive = authority.is_some_and(|(_, competitive)| competitive);
            if authority.is_none() || !startup_status_allows(&plan.tasks[task_index]) {
                return Err(Self::task_readback_error(
                    "task_dispatch_ack",
                    "attempt no longer owns the acknowledging tab",
                ));
            }
            if plan.tasks[task_index].status == crate::plan::TaskStatus::Pending {
                progression_authority(plan, task_index, competitive, attempt_id)?;
            }
            let task = &mut plan.tasks[task_index];
            let attempt = &mut task.execution_attempts[attempt_index];
            attempt.acknowledged_at.get_or_insert(now_ms);
            if task.status != crate::plan::TaskStatus::InProgress {
                task.status_changed_at = now_ms;
            }
            task.status = crate::plan::TaskStatus::InProgress;
            task.status_label = Some(if competitive {
                "competitive".to_string()
            } else {
                "acknowledged".to_string()
            });
            task.updated_at = now_ms;
            acknowledged = Some(attempt.clone());
            Ok(())
        });
        let attempt = acknowledged.ok_or_else(|| {
            Self::task_readback_error("task_dispatch_ack", "attempt was not acknowledged")
        })?;
        let verified = (|| {
            let task = self.read_task_for_verification(plan_id, task_id)?;
            let persisted = task
                .execution_attempts
                .iter()
                .find(|candidate| candidate.id == attempt_id)
                .ok_or_else(|| Self::task_readback_error("task_dispatch_ack", "attempt missing"))?;
            let executor_persisted = if competitive {
                task.status_label.as_deref() == Some("competitive")
                    && super::super::mutations::live_competitive_executor(&task, attempt_id)
            } else {
                task.status_label.as_deref() == Some("acknowledged")
                    && task.assigned_tab_id.as_deref() == Some(tab_id)
            };
            if persisted.executor_tab_id != tab_id
                || persisted.acknowledged_at != attempt.acknowledged_at
                || task.status != crate::plan::TaskStatus::InProgress
                || !executor_persisted
            {
                return Err(Self::task_readback_error(
                    "task_dispatch_ack",
                    format!("acknowledged dispatch state did not match for {}", task.id),
                ));
            }
            Ok(())
        })();
        pending_after_verified(write, verified, Some(task_id.to_string()))?;
        Ok(super::super::task_attempts::TaskAttemptAcknowledgement {
            plan_id: plan_id.to_string(),
            task_id: task_id.to_string(),
            attempt,
            already_acknowledged: false,
        })
    }

    /// Every live attempt this tab executes, whatever the plan or task state.
    /// Callers apply their own acceptance rule and report what they rejected.
    pub fn live_task_attempts_for_tab(&self, tab_id: &str) -> Vec<LiveTabAttempt> {
        self.list_full()
            .into_iter()
            .flat_map(|plan| {
                let plan_id = plan.id.clone();
                let plan_active = plan.status == crate::plan::PlanStatus::Active;
                plan.tasks.into_iter().filter_map(move |task| {
                    task.execution_attempts
                        .iter()
                        .rfind(|attempt| {
                            attempt.executor_tab_id == tab_id && attempt.closed_at.is_none()
                        })
                        .map(|attempt| LiveTabAttempt {
                            plan_id: plan_id.clone(),
                            task_id: task.id.clone(),
                            task_handle: task.handle.clone(),
                            plan_active,
                            task_status: task.status.clone(),
                            attempt: attempt.clone(),
                        })
                })
            })
            .collect()
    }

    /// A closed attempt this tab owned on a task that is live on another tab.
    /// Distinguishes an orphan of a superseded dispatch from a stray shell.
    pub fn superseded_attempt_for_tab(
        &self,
        tab_id: &str,
    ) -> Option<(String, String, String, String, String)> {
        self.list_full().into_iter().find_map(|plan| {
            let plan_id = plan.id.clone();
            plan.tasks.into_iter().find_map(|task| {
                let (owned_index, owned) = task
                    .execution_attempts
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, attempt)| attempt.executor_tab_id == tab_id)?;
                owned.closed_at?;
                let live = task
                    .execution_attempts
                    .iter()
                    .enumerate()
                    .skip(owned_index + 1)
                    .rev()
                    .find(|(_, attempt)| {
                        attempt.closed_at.is_none() && attempt.retired_at.is_none()
                    })?;
                Some((
                    plan_id.clone(),
                    task.id.clone(),
                    owned.id.clone(),
                    live.1.id.clone(),
                    live.1.executor_tab_id.clone(),
                ))
            })
        })
    }

    /// The tab binding selects the subject; `plan_hint`/`task_hint` come from the
    /// Worker's environment and only break a tie, so a stale pair cannot fence a
    /// Worker out of acknowledging the attempt it actually owns.
    pub fn acknowledge_live_task_attempt_for_tab(
        &self,
        tab_id: &str,
        plan_hint: Option<&str>,
        task_hint: Option<&str>,
        now_ms: u64,
    ) -> Result<super::super::task_attempts::TaskAttemptAcknowledgement, PlanError> {
        let bound = self.live_task_attempts_for_tab(tab_id);
        let acknowledgeable = bound
            .iter()
            .filter(|candidate| {
                candidate.plan_active
                    && (candidate.task_status.counts_as_open_work()
                        || candidate.attempt.acknowledged_at.is_some())
            })
            .collect::<Vec<_>>();
        let selected = match acknowledgeable.len() {
            1 => acknowledgeable[0],
            0 => {
                let cause = match self.superseded_attempt_for_tab(tab_id) {
                    Some((plan_id, task_id, previous_attempt_id, current_attempt_id, live_tab))
                        if bound.is_empty() =>
                    {
                        superseded_subject_cause(
                            tab_id,
                            &plan_id,
                            &task_id,
                            &previous_attempt_id,
                            &current_attempt_id,
                            &live_tab,
                        )
                    }
                    _ => unresolved_subject_cause(tab_id, &bound),
                };
                return Err(Self::task_readback_error("task_dispatch_ack", cause));
            }
            _ => {
                let hinted = acknowledgeable
                    .iter()
                    .copied()
                    .filter(|candidate| candidate.matches_subject_hint(plan_hint, task_hint))
                    .collect::<Vec<_>>();
                if hinted.len() != 1 {
                    return Err(Self::task_readback_error(
                        "task_dispatch_ack",
                        ambiguous_subject_cause(tab_id, acknowledgeable.iter().copied()),
                    ));
                }
                hinted[0]
            }
        };
        self.acknowledge_task_attempt(
            &selected.plan_id,
            &selected.task_id,
            tab_id,
            &selected.attempt.id,
            now_ms,
        )
    }

    pub fn mark_task_attempt_runtime_ready(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
        session_id: &str,
        now_ms: u64,
    ) -> Result<super::super::task_attempts::TaskAttemptCreated, PlanError> {
        let mut ready = None;
        let write = self.with_plan(plan_id, now_ms, |plan| {
            let task_index = crate::plan::task_key::find_task_index(&plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let attempt_index = plan.tasks[task_index]
                .execution_attempts
                .iter()
                .position(|attempt| attempt.id == attempt_id)
                .ok_or(PlanError::TaskNotFound)?;
            let acknowledged = plan.tasks[task_index].execution_attempts[attempt_index]
                .acknowledged_at
                .is_some();
            if acknowledged && plan.tasks[task_index].status != crate::plan::TaskStatus::InProgress
            {
                let competitive = plan.tasks[task_index]
                    .execution_attempts
                    .iter()
                    .any(|attempt| {
                        attempt.id == attempt_id && attempt.competition_group_id.is_some()
                    });
                progression_authority(plan, task_index, competitive, attempt_id)?;
            }
            let task = &mut plan.tasks[task_index];
            let attempt = &mut task.execution_attempts[attempt_index];
            if attempt.executor_tab_id != tab_id
                || attempt.closed_at.is_some()
                || attempt.retired_at.is_some()
            {
                return Err(Self::task_readback_error(
                    "task_attempt_runtime_ready",
                    "attempt no longer owns the ready tab",
                ));
            }
            attempt.session_id = Some(session_id.to_string());
            if acknowledged {
                if task.status != crate::plan::TaskStatus::InProgress {
                    task.status_changed_at = now_ms;
                }
                task.status = crate::plan::TaskStatus::InProgress;
                task.status_label = Some("acknowledged".into());
            } else if attempt.competition_group_id.is_none() {
                task.status_label = Some("awaiting_worker_ack".into());
            }
            task.updated_at = now_ms;
            ready = Some(attempt.clone());
            Ok(())
        });
        let attempt = ready.ok_or_else(|| {
            Self::task_readback_error("task_attempt_runtime_ready", "attempt was not updated")
        })?;
        let verified = (|| {
            let task = self.read_task_for_verification(plan_id, task_id)?;
            let persisted = task
                .execution_attempts
                .iter()
                .find(|candidate| candidate.id == attempt_id)
                .ok_or_else(|| {
                    Self::task_readback_error("task_attempt_runtime_ready", "attempt missing")
                })?;
            let executor_persisted = persisted.acknowledged_at.is_none()
                || (task.status == crate::plan::TaskStatus::InProgress
                    && (persisted.competition_group_id.is_none()
                        || super::super::mutations::live_competitive_executor(&task, attempt_id)));
            if persisted.executor_tab_id != tab_id
                || persisted.session_id.as_deref() != Some(session_id)
                || !executor_persisted
            {
                return Err(Self::task_readback_error(
                    "task_attempt_runtime_ready",
                    format!("runtime-ready attempt did not match for {}", task.id),
                ));
            }
            Ok(())
        })();
        pending_after_verified(write, verified, Some(task_id.to_string()))?;
        Ok(super::super::task_attempts::TaskAttemptCreated {
            plan_id: plan_id.to_string(),
            task_id: task_id.to_string(),
            attempt,
        })
    }

    pub fn mark_live_task_attempt_runtime_ready_for_tab(
        &self,
        tab_id: &str,
        session_id: &str,
        now_ms: u64,
    ) -> Result<Option<super::super::task_attempts::TaskAttemptCreated>, PlanError> {
        let matches = self
            .list_full()
            .into_iter()
            .filter(|plan| plan.status == crate::plan::PlanStatus::Active)
            .flat_map(|plan| {
                let plan_id = plan.id.clone();
                plan.tasks
                    .into_iter()
                    .filter(|task| {
                        matches!(
                            task.status,
                            crate::plan::TaskStatus::Pending | crate::plan::TaskStatus::InProgress
                        )
                    })
                    .filter_map(move |task| {
                        task.execution_attempts
                            .iter()
                            .rfind(|attempt| {
                                attempt.executor_tab_id == tab_id && attempt.closed_at.is_none()
                            })
                            .map(|attempt| (plan_id.clone(), task.id, attempt.id.clone()))
                    })
            })
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [] => Ok(None),
            [(plan_id, task_id, attempt_id)] => self
                .mark_task_attempt_runtime_ready(
                    plan_id, task_id, tab_id, attempt_id, session_id, now_ms,
                )
                .map(Some),
            _ => Err(Self::task_readback_error(
                "task_attempt_runtime_ready",
                format!(
                    "expected at most one live task attempt for tab {tab_id}, found {}",
                    matches.len()
                ),
            )),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit_task_dispatch(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        executor_title: &str,
        session_id: &str,
        mut lane: crate::plan::TaskLaneMetadata,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let write = self.with_plan(plan_id, now_ms, |plan| {
            let task_index = crate::plan::task_key::find_task_index(&plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            lane.tab_id = Some(tab_id.to_string());
            let lane_update = super::super::TaskLaneUpdate {
                tab_id: Some(Some(tab_id.to_string())),
                ..Default::default()
            };
            super::super::mutations::validate_task_status_transition_with_lane(
                plan,
                &plan.tasks[task_index].id,
                &plan.tasks[task_index].status,
                &crate::plan::TaskStatus::InProgress,
                None,
                Some(&lane_update),
            )?;
            let task = &mut plan.tasks[task_index];
            task.lane = normalize_lane_metadata(lane);
            task.assigned_tab_id = Some(tab_id.to_string());
            task.taken_by_role = Some("worker".into());
            task.peer = None;
            let mut attempt = super::super::task_attempts::new_assignment_attempt(
                task,
                tab_id,
                None,
                Some("worker"),
                now_ms,
            );
            attempt.executor_title = Some(executor_title.to_string());
            attempt.session_id = Some(session_id.to_string());
            super::super::task_attempts::record_noncompetitive_replacement_attempt(
                &mut task.execution_attempts,
                attempt,
                now_ms,
            );
            task.status = crate::plan::TaskStatus::InProgress;
            task.status_changed_at = now_ms;
            task.updated_at = now_ms;
            plan.mode = crate::plan::PlanMode::Execution;
            Ok(())
        });
        let verified = (|| {
            let task = self.read_task_for_verification(plan_id, task_id)?;
            let attempt = task
                .execution_attempts
                .last()
                .ok_or_else(|| Self::task_readback_error("task_dispatch", "attempt missing"))?;
            if task.status != crate::plan::TaskStatus::InProgress
                || task.assigned_tab_id.as_deref() != Some(tab_id)
                || task.lane.tab_id.as_deref() != Some(tab_id)
                || attempt.executor_tab_id != tab_id
                || attempt.executor_title.as_deref() != Some(executor_title)
                || attempt.session_id.as_deref() != Some(session_id)
                || attempt.worktree_path != task.lane.worktree_path
                || attempt.branch != task.lane.branch
                || attempt.base != task.lane.base
                || attempt.destination != task.lane.destination
            {
                return Err(Self::task_readback_error(
                    "task_dispatch",
                    format!("dispatch state did not match for {}", task.id),
                ));
            }
            Ok(())
        })();
        pending_after_verified(write, verified, Some(task_id.to_string()))
    }
}
