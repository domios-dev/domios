use super::lane_metadata::normalize_lane_metadata;
use super::readback::pending_after_verified;
use super::{PlanError, PlanStore};

impl PlanStore {
    #[allow(clippy::too_many_arguments)]
    pub fn begin_task_dispatch(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        executor_title: &str,
        mut lane: crate::plan::TaskLaneMetadata,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let mut attempt_id = None;
        let mut original_status = None;
        let write = self.with_plan(plan_id, now_ms, |plan| {
            let task_index = plan
                .tasks
                .iter()
                .position(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            lane.tab_id = Some(tab_id.to_string());
            let lane_update = super::TaskLaneUpdate {
                tab_id: Some(Some(tab_id.to_string())),
                ..Default::default()
            };
            super::mutations::validate_task_status_transition_with_lane(
                plan,
                &plan.tasks[task_index].id,
                &plan.tasks[task_index].status,
                &crate::plan::TaskStatus::InProgress,
                None,
                Some(&lane_update),
            )?;
            original_status = Some(plan.tasks[task_index].status.clone());
            let task = &mut plan.tasks[task_index];
            task.lane = normalize_lane_metadata(lane);
            task.assigned_tab_id = Some(tab_id.to_string());
            task.taken_by_role = Some("worker".into());
            task.peer = None;
            let mut attempt = super::task_attempts::new_assignment_attempt(
                task,
                tab_id,
                None,
                Some("worker"),
                now_ms,
            );
            attempt.executor_title = Some(executor_title.to_string());
            attempt_id = Some(attempt.id.clone());
            task.execution_attempts.push(attempt);
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
                || attempt.session_id.is_some()
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

    #[allow(clippy::too_many_arguments)]
    pub fn finish_task_dispatch(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
        session_id: &str,
        mut lane: crate::plan::TaskLaneMetadata,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let write = self.with_plan(plan_id, now_ms, |plan| {
            let task_index = plan
                .tasks
                .iter()
                .position(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
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
                super::mutations::validate_task_status_transition(
                    plan,
                    &plan.tasks[task_index].id,
                    &plan.tasks[task_index].status,
                    &crate::plan::TaskStatus::InProgress,
                    None,
                )?;
            }
            let task = &mut plan.tasks[task_index];
            if task.assigned_tab_id.as_deref() != Some(tab_id) {
                return Err(Self::task_readback_error(
                    "task_dispatch_finish",
                    "assigned tab changed before readiness",
                ));
            }
            lane.tab_id = Some(tab_id.to_string());
            task.lane = normalize_lane_metadata(lane);
            let attempt = &mut task.execution_attempts[attempt_index];
            if attempt.executor_tab_id != tab_id || attempt.closed_at.is_some() {
                return Err(Self::task_readback_error(
                    "task_dispatch_finish",
                    "starting attempt no longer owns the tab",
                ));
            }
            attempt.session_id = Some(session_id.to_string());
            attempt.worktree_path = task.lane.worktree_path.clone();
            attempt.branch = task.lane.branch.clone();
            attempt.base = task.lane.base.clone();
            attempt.destination = task.lane.destination.clone();
            if attempt.acknowledged_at.is_some() {
                task.status = crate::plan::TaskStatus::InProgress;
                task.status_label = None;
                task.status_changed_at = now_ms;
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
                task.status == crate::plan::TaskStatus::InProgress && task.status_label.is_none()
            } else {
                task.status_label.as_deref() == Some("awaiting_worker_ack")
            };
            if !ready_state
                || task.assigned_tab_id.as_deref() != Some(tab_id)
                || attempt.session_id.as_deref() != Some(session_id)
                || attempt.worktree_path != task.lane.worktree_path
                || attempt.branch != task.lane.branch
                || attempt.base != task.lane.base
                || attempt.destination != task.lane.destination
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
    ) -> Result<super::task_attempts::TaskAttemptCreated, PlanError> {
        let mut acknowledged = None;
        let write = self.with_plan(plan_id, now_ms, |plan| {
            let task_index = plan
                .tasks
                .iter()
                .position(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            let attempt_index = plan.tasks[task_index]
                .execution_attempts
                .iter()
                .position(|attempt| attempt.id == attempt_id)
                .ok_or(PlanError::TaskNotFound)?;
            let attempt = &plan.tasks[task_index].execution_attempts[attempt_index];
            if attempt.executor_tab_id != tab_id || attempt.closed_at.is_some() {
                return Err(Self::task_readback_error(
                    "task_dispatch_ack",
                    "attempt no longer owns the acknowledging tab",
                ));
            }
            if attempt.session_id.is_some()
                && plan.tasks[task_index].status != crate::plan::TaskStatus::InProgress
            {
                super::mutations::validate_task_status_transition(
                    plan,
                    &plan.tasks[task_index].id,
                    &plan.tasks[task_index].status,
                    &crate::plan::TaskStatus::InProgress,
                    None,
                )?;
            }
            let task = &mut plan.tasks[task_index];
            let attempt = &mut task.execution_attempts[attempt_index];
            attempt.acknowledged_at.get_or_insert(now_ms);
            if attempt.session_id.is_some() {
                if task.status != crate::plan::TaskStatus::InProgress {
                    task.status_changed_at = now_ms;
                }
                task.status = crate::plan::TaskStatus::InProgress;
                task.status_label = attempt
                    .competition_group_id
                    .as_ref()
                    .map(|_| "competitive".to_string());
            }
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
            let expected_label = persisted
                .competition_group_id
                .as_ref()
                .map(|_| "competitive");
            if persisted.executor_tab_id != tab_id
                || persisted.acknowledged_at != attempt.acknowledged_at
                || (persisted.session_id.is_some()
                    && (task.status != crate::plan::TaskStatus::InProgress
                        || task.status_label.as_deref() != expected_label))
            {
                return Err(Self::task_readback_error(
                    "task_dispatch_ack",
                    format!("acknowledged dispatch state did not match for {}", task.id),
                ));
            }
            Ok(())
        })();
        pending_after_verified(write, verified, Some(task_id.to_string()))?;
        Ok(super::task_attempts::TaskAttemptCreated {
            plan_id: plan_id.to_string(),
            task_id: task_id.to_string(),
            attempt,
        })
    }

    pub fn acknowledge_live_task_attempt_for_tab(
        &self,
        tab_id: &str,
        plan_id: Option<&str>,
        task_id: Option<&str>,
        now_ms: u64,
    ) -> Result<super::task_attempts::TaskAttemptCreated, PlanError> {
        let matches = self
            .list_full()
            .into_iter()
            .filter(|plan| plan.status == crate::plan::PlanStatus::Active)
            .filter(|plan| plan_id.is_none_or(|expected| plan.id == expected))
            .flat_map(|plan| {
                let plan_id = plan.id.clone();
                plan.tasks
                    .into_iter()
                    .filter(move |task| {
                        task.status.counts_as_open_work()
                            && task_id.is_none_or(|expected| {
                                task.id == expected || task.handle.as_deref() == Some(expected)
                            })
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
        let [(plan_id, task_id, attempt_id)] = matches.as_slice() else {
            return Err(Self::task_readback_error(
                "task_dispatch_ack",
                format!(
                    "expected one live task attempt for tab {tab_id}, found {}",
                    matches.len()
                ),
            ));
        };
        self.acknowledge_task_attempt(plan_id, task_id, tab_id, attempt_id, now_ms)
    }

    pub fn mark_task_attempt_runtime_ready(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
        session_id: &str,
        now_ms: u64,
    ) -> Result<super::task_attempts::TaskAttemptCreated, PlanError> {
        let mut ready = None;
        let write = self.with_plan(plan_id, now_ms, |plan| {
            let task_index = plan
                .tasks
                .iter()
                .position(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
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
                super::mutations::validate_task_status_transition(
                    plan,
                    &plan.tasks[task_index].id,
                    &plan.tasks[task_index].status,
                    &crate::plan::TaskStatus::InProgress,
                    None,
                )?;
            }
            let task = &mut plan.tasks[task_index];
            let attempt = &mut task.execution_attempts[attempt_index];
            if attempt.executor_tab_id != tab_id || attempt.closed_at.is_some() {
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
                if attempt.competition_group_id.is_none() {
                    task.status_label = None;
                }
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
            if persisted.executor_tab_id != tab_id
                || persisted.session_id.as_deref() != Some(session_id)
                || (persisted.acknowledged_at.is_some()
                    && task.status != crate::plan::TaskStatus::InProgress)
            {
                return Err(Self::task_readback_error(
                    "task_attempt_runtime_ready",
                    format!("runtime-ready attempt did not match for {}", task.id),
                ));
            }
            Ok(())
        })();
        pending_after_verified(write, verified, Some(task_id.to_string()))?;
        Ok(super::task_attempts::TaskAttemptCreated {
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
    ) -> Result<Option<super::task_attempts::TaskAttemptCreated>, PlanError> {
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
            let task_index = plan
                .tasks
                .iter()
                .position(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            lane.tab_id = Some(tab_id.to_string());
            let lane_update = super::TaskLaneUpdate {
                tab_id: Some(Some(tab_id.to_string())),
                ..Default::default()
            };
            super::mutations::validate_task_status_transition_with_lane(
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
            let mut attempt = super::task_attempts::new_assignment_attempt(
                task,
                tab_id,
                None,
                Some("worker"),
                now_ms,
            );
            attempt.executor_title = Some(executor_title.to_string());
            attempt.session_id = Some(session_id.to_string());
            task.execution_attempts.push(attempt);
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

    pub fn restore_task_dispatch_snapshot(
        &self,
        plan_id: &str,
        snapshot: &crate::plan::Task,
        plan_mode: crate::plan::PlanMode,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let snapshot = snapshot.clone();
        let task_id = snapshot.id.clone();
        let expected = snapshot.clone();
        let expected_mode = plan_mode.clone();
        let write = self.with_plan(plan_id, now_ms, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id || task.handle.as_deref() == Some(&task_id))
                .ok_or(PlanError::TaskNotFound)?;
            *task = snapshot;
            plan.mode = plan_mode;
            Ok(())
        });
        let verified = self
            .committed_plan_for(plan_id)
            .ok_or(PlanError::NotFound)
            .and_then(|plan| {
                let mode_matches = plan.mode == expected_mode;
                let task = plan
                    .tasks
                    .into_iter()
                    .find(|task| task.id == task_id || task.handle.as_deref() == Some(&task_id))
                    .ok_or(PlanError::TaskNotFound)?;
                if task == expected && mode_matches {
                    Ok(())
                } else {
                    Err(Self::task_readback_error(
                        "task_dispatch_restore",
                        format!("task snapshot did not match for {task_id}"),
                    ))
                }
            });
        pending_after_verified(write, verified, Some(task_id))
    }
}
