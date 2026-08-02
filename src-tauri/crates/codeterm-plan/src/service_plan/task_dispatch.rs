use super::lane_metadata::normalize_lane_metadata;
use super::readback::pending_after_verified;
use super::{PlanError, PlanStore};

fn startup_attempt<'a>(
    task: &'a crate::plan::Task,
    tab_id: &str,
    attempt_id: &str,
) -> Option<(&'a crate::plan::TaskExecutionAttempt, bool)> {
    let attempt = task.execution_attempts.iter().find(|attempt| {
        attempt.id == attempt_id && attempt.executor_tab_id == tab_id && attempt.closed_at.is_none()
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

fn startup_status_allows(task: &crate::plan::Task, competitive: bool) -> bool {
    task.status == crate::plan::TaskStatus::Pending
        || (competitive && task.status == crate::plan::TaskStatus::InProgress)
}

impl PlanStore {
    pub fn worker_startup_attempt_owns(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
        session_id: Option<&str>,
    ) -> bool {
        self.get(plan_id).is_some_and(|plan| {
            plan.tasks
                .iter()
                .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
                .and_then(|task| {
                    startup_attempt(task, tab_id, attempt_id)
                        .map(|(attempt, competitive)| (task, attempt, competitive))
                })
                .is_some_and(|(task, attempt, competitive)| {
                    startup_status_allows(task, competitive)
                        && attempt.acknowledged_at.is_none()
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
        let owns_startup = authority.is_some_and(|(attempt, competitive)| {
            startup_status_allows(&current, competitive) && attempt.acknowledged_at.is_none()
        });
        if !owns_startup {
            return Ok(false);
        }
        self.with_plan(plan_id, now_ms, |plan| {
            let Some(task) = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
            else {
                return Ok(false);
            };
            let authority = startup_attempt(task, tab_id, attempt_id);
            let owns_startup = authority.is_some_and(|(attempt, competitive)| {
                startup_status_allows(task, competitive) && attempt.acknowledged_at.is_none()
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
        self.begin_task_dispatch_with_policy_inner(
            plan_id,
            task_id,
            tab_id,
            executor_title,
            lane,
            intent,
            allowance,
            provider,
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
        self.begin_task_dispatch_with_policy_inner(
            plan_id,
            task_id,
            tab_id,
            executor_title,
            lane,
            intent,
            allowance,
            provider,
            Some(attempt_id),
            now_ms,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn begin_task_dispatch_with_policy_inner(
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
        now_ms: u64,
    ) -> Result<String, PlanError> {
        super::interruption::provider_allowance_preflight(&intent, allowance.as_ref(), now_ms)?;
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
            let mut attempt = super::task_attempts::new_assignment_attempt_with_id(
                task,
                tab_id,
                None,
                Some("worker"),
                reserved_attempt_id,
                now_ms,
            );
            attempt.executor_title = Some(executor_title.to_string());
            attempt.provider = provider.map(str::to_string);
            attempt.agent_type = provider.map(str::to_string);
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
                || attempt.session_id.is_some()
                || task.interruption.execution_intent != intent
                || task.interruption.allowance != allowance
                || task.interruption.owner_tab_id.as_deref() != Some(tab_id)
                || attempt.interruption.owner_epoch != task.interruption.owner_epoch
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
        session_binding_nonce: &str,
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
            attempt.session_binding_nonce = Some(session_binding_nonce.to_string());
            task.interruption.owner_session_id = Some(session_id.to_string());
            task.interruption.owner_tab_id = Some(tab_id.to_string());
            task.interruption.owner_epoch = task.interruption.owner_epoch.max(1);
            attempt.interruption.owner_epoch = task.interruption.owner_epoch;
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
                || attempt.session_binding_nonce.as_deref() != Some(session_binding_nonce)
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
            if current_task.status != crate::plan::TaskStatus::InProgress {
                return Err(Self::task_readback_error(
                    "task_dispatch_ack",
                    "acknowledged attempt no longer owns an acknowledged task",
                ));
            }
            let attempt = current_attempt.clone();
            return Ok(super::task_attempts::TaskAttemptCreated {
                plan_id: plan_id.to_string(),
                task_id: current_task.id,
                attempt,
            });
        }
        if !startup_status_allows(&current_task, competitive) {
            return Err(Self::task_readback_error(
                "task_dispatch_ack",
                "only a pending startup assignment can be acknowledged",
            ));
        }
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
            let authority = startup_attempt(&plan.tasks[task_index], tab_id, attempt_id);
            let competitive = authority.is_some_and(|(_, competitive)| competitive);
            if authority.is_none() || !startup_status_allows(&plan.tasks[task_index], competitive) {
                return Err(Self::task_readback_error(
                    "task_dispatch_ack",
                    "attempt no longer owns the acknowledging tab",
                ));
            }
            if plan.tasks[task_index].status == crate::plan::TaskStatus::Pending {
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
            if persisted.executor_tab_id != tab_id
                || persisted.acknowledged_at != attempt.acknowledged_at
                || task.status != crate::plan::TaskStatus::InProgress
                || (!competitive && task.status_label.as_deref() != Some("acknowledged"))
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
            super::task_attempts::record_noncompetitive_replacement_attempt(
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
