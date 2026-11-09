use super::*;
use std::sync::Arc;

impl ServiceContext {
    #[allow(clippy::too_many_arguments)]
    pub fn arm_worker_ack_deadline(
        &self,
        parent_tab_id: &str,
        worker_tab_id: &str,
        worker_title: &str,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        spawn_command: String,
    ) {
        let armed = (|| -> Result<(), crate::service::plan::PlanError> {
            let Some(attempt) = self.plan_store.worker_ack_deadline_attempt_projection(
                plan_id,
                task_id,
                worker_tab_id,
                attempt_id,
            )?
            else {
                return Ok(());
            };
            let identity =
                self.worker_ack_identity_for_attempt(plan_id, task_id, worker_tab_id, &attempt);
            if !self.worker_ack_identity_is_current(&identity)? {
                return Ok(());
            }
            let arm = self.worker_ack.arm(identity.clone());
            if arm.identity_conflict {
                log::warn!(
                    "[worker-ack] conflicting waiter identity task={} attempt={} tab={}",
                    task_id,
                    attempt_id,
                    worker_tab_id
                );
                return Ok(());
            }
            if !arm.starts_supervisor {
                return Ok(());
            }
            let Some(receiver) = arm.receiver else {
                return Ok(());
            };
            let service = Arc::new((*self).clone());
            let parent_tab_id = parent_tab_id.to_string();
            let worker_tab_id = worker_tab_id.to_string();
            let worker_title = worker_title.to_string();
            let plan_id = plan_id.to_string();
            let task_id = task_id.to_string();
            let attempt_id = attempt_id.to_string();
            std::thread::spawn(move || {
                let supervised = (|| -> Result<(), crate::service::plan::PlanError> {
                    if await_worker_ack(&receiver, WORKER_ACK_TIMEOUT)
                        == WorkerAckWaitOutcome::Resolved
                    {
                        return Ok(());
                    }
                    service.reconcile_tool_less_worker_ack(&worker_tab_id);
                    if !service.worker_ack_identity_is_current(&identity)? {
                        return Ok(());
                    }
                    let authenticated = service.authenticated_worker_binding(&worker_tab_id);
                    let reminder_allowed = match authenticated.as_ref() {
                        Some((session_id, nonce))
                            if identity.session_id.as_deref() == Some(session_id)
                                && identity.binding_nonce.as_deref() == Some(nonce) =>
                        {
                            service.live_unacknowledged_attempt_owns_tab(
                                &plan_id,
                                &task_id,
                                &worker_tab_id,
                                &attempt_id,
                                Some(session_id),
                            )?
                        }
                        _ => false,
                    };
                    service.observe_tab_state(
                        &worker_tab_id,
                        crate::service::supporter_tab_state::ObservationTrigger::StartupEscalation,
                        "worker_ack_startup_deadline",
                    );
                    service.nudge_unacknowledged_startup(&worker_tab_id, &attempt_id);
                    if !reminder_allowed {
                        service.plan_store.mark_worker_startup_deadline(
                            &plan_id,
                            &task_id,
                            &worker_tab_id,
                            &attempt_id,
                            false,
                            epoch_ms(),
                        )?;
                    }
                    if await_worker_ack(&receiver, WORKER_ACK_FINAL_TIMEOUT)
                        == WorkerAckWaitOutcome::Resolved
                    {
                        return Ok(());
                    }
                    service.reconcile_tool_less_worker_ack(&worker_tab_id);
                    if !service.worker_ack_identity_is_current(&identity)? {
                        return Ok(());
                    }
                    if !service.worker_ack.expire(&identity) {
                        return Ok(());
                    }
                    if !service.worker_ack_identity_is_current(&identity)? {
                        return Ok(());
                    }
                    let finalized = service.plan_store.mark_worker_startup_deadline(
                        &plan_id,
                        &task_id,
                        &worker_tab_id,
                        &attempt_id,
                        true,
                        epoch_ms(),
                    )?;
                    if !finalized {
                        return Ok(());
                    }
                    service.nudge_unacknowledged_startup(&worker_tab_id, &attempt_id);
                    let mut event = worker_ack_timeout_event(
                        &plan_id,
                        &task_id,
                        &worker_tab_id,
                        &attempt_id,
                        &spawn_command,
                    );
                    if let Some(cause) =
                        service.worker_ack_failure_cause(&worker_tab_id, &attempt_id)
                    {
                        event["failure_reason"] = cause.into();
                    }
                    service.publish_worker_lifecycle_report(
                        &parent_tab_id,
                        &worker_tab_id,
                        &worker_title,
                        &plan_id,
                        &task_id,
                        event,
                    );
                    Ok(())
                })();
                if let Err(error) = supervised {
                    log::error!("[worker-ack] startup supervision storage failure task={task_id} attempt={attempt_id} tab={worker_tab_id}: {error}");
                }
            });
            Ok(())
        })();
        if let Err(error) = armed {
            log::error!("[worker-ack] deadline admission storage failure task={task_id} attempt={attempt_id} tab={worker_tab_id}: {error}");
        }
    }

    pub fn arm_bound_worker_ack_deadline_for_tab(
        &self,
        tab_id: &str,
        spawn_command: String,
    ) -> Result<bool, String> {
        use crate::service::plan::{ambiguous_subject_cause, unresolved_subject_cause};
        let bound = self
            .plan_store
            .live_task_attempts_for_tab(tab_id)
            .map_err(|error| error.to_string())?;
        if bound.is_empty() {
            return Ok(false);
        }
        let armable = bound
            .iter()
            .filter(|candidate| {
                candidate.plan_active
                    && matches!(
                        candidate.task_status,
                        crate::plan::TaskStatus::Pending | crate::plan::TaskStatus::InProgress
                    )
            })
            .collect::<Vec<_>>();
        let selected = match armable.len() {
            1 => armable[0],
            0 => return Err(unresolved_subject_cause(tab_id, &bound)),
            _ => return Err(ambiguous_subject_cause(tab_id, armable.iter().copied())),
        };
        let (plan_id, task_id, attempt) = (&selected.plan_id, &selected.task_id, &selected.attempt);
        let parent_tab_id = attempt
            .orchestrator_tab_id
            .as_deref()
            .or(attempt.parent_tab_id.as_deref())
            .map(str::to_string)
            .or_else(|| {
                self.mux
                    .read()
                    .get_tab(tab_id)
                    .and_then(|tab| tab.parent_tab_id.clone())
            })
            .ok_or_else(|| format!("task-bound Worker {tab_id} has no report target"))?;
        let title = self
            .mux
            .read()
            .get_tab(tab_id)
            .map(|tab| tab.title.clone())
            .unwrap_or_else(|| "Worker".into());
        self.arm_worker_ack_deadline(
            &parent_tab_id,
            tab_id,
            &title,
            plan_id,
            task_id,
            &attempt.id,
            spawn_command,
        );
        Ok(true)
    }
}
