use super::*;

impl ServiceContext {
    pub(crate) fn arm_agent_startup_deadline(
        &self,
        authority: StartupAuthority,
        agent_title: &str,
    ) {
        let receiver = match self.worker_ack.arm_startup(authority.clone()) {
            AgentStartupArm::Armed(receiver) => receiver,
            AgentStartupArm::Extended | AgentStartupArm::AlreadyAcknowledged => return,
        };
        let supervisor_tab_id = authority.creator_tab_id.clone();
        let agent_tab_id = authority.child_tab_id.clone();
        let attempt_id = authority.attempt_id.clone();
        let service = self.clone();
        let agent_title = agent_title.to_string();
        std::thread::spawn(move || {
            let mut generation = 0;
            let mut exit_code = match await_agent_startup_notice(
                &receiver,
                AGENT_STARTUP_NOTICE_TIMEOUT,
                &mut generation,
            ) {
                AgentStartupNoticeOutcome::Resolved => return,
                AgentStartupNoticeOutcome::Overdue { exit_code } => exit_code,
            };
            if !service.startup_attempt_is_current(&agent_tab_id, &attempt_id) {
                return;
            }
            service.nudge_unacknowledged_startup(&agent_tab_id, &attempt_id);
            service.publish_agent_startup_report(
                &supervisor_tab_id,
                &agent_tab_id,
                &agent_title,
                agent_startup_notice_event(&agent_tab_id, &agent_title, &attempt_id),
            );
            if !service.startup_attempt_is_current(&agent_tab_id, &attempt_id) {
                return;
            }
            loop {
                match await_agent_startup_notice(
                    &receiver,
                    AGENT_STARTUP_FINAL_TIMEOUT,
                    &mut generation,
                ) {
                    AgentStartupNoticeOutcome::Resolved => return,
                    AgentStartupNoticeOutcome::Overdue {
                        exit_code: finished,
                    } => {
                        if finished.is_some() {
                            exit_code = finished;
                        }
                    }
                }
                match service.worker_ack.expire_startup(&authority, generation) {
                    WaiterExpiry::Expired => break,
                    WaiterExpiry::Extended => continue,
                    WaiterExpiry::NoLongerCurrent => return,
                }
            }
            if !service.startup_attempt_is_current(&agent_tab_id, &attempt_id) {
                return;
            }
            let provider_output = service.startup_provider_output(&attempt_id);
            service.publish_agent_startup_report(
                &supervisor_tab_id,
                &agent_tab_id,
                &agent_title,
                agent_startup_failed_event(
                    &agent_tab_id,
                    &agent_title,
                    &attempt_id,
                    exit_code,
                    provider_output.as_deref(),
                ),
            );
        });
    }

    fn live_unacknowledged_attempt_owns_tab(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
        session_id: Option<&str>,
    ) -> bool {
        self.plan_store
            .worker_startup_attempt_owns(plan_id, task_id, tab_id, attempt_id, session_id)
    }

    fn authenticated_worker_binding(&self, tab_id: &str) -> Option<(String, String)> {
        self.mux.read().get_tab(tab_id).and_then(|tab| {
            super::task_assignment::authenticated_tab_binding(tab)
                .map(|binding| (binding.session_id, binding.session_binding_nonce))
        })
    }

    /// Nudge a tab that has not acknowledged its startup. The tab's own state
    /// decides whether the nudge is this text at all: a terminal still owned by
    /// a provider dialog gets that dialog answered instead, and a terminal with
    /// no agent behind it gets nothing.
    pub(crate) fn nudge_unacknowledged_startup(&self, tab_id: &str, attempt_id: &str) {
        if !self.classify_startup_nudge(tab_id).delivers() {
            return;
        }
        if let Err(error) = self.deliver_worker_ack_reminder(tab_id, attempt_id) {
            log::warn!("[worker-ack] reminder delivery failed attempt={attempt_id}: {error}");
        }
    }

    pub(crate) fn deliver_worker_ack_reminder(
        &self,
        tab_id: &str,
        attempt_id: &str,
    ) -> Result<(), String> {
        self.gateway
            .submit(self, worker_ack_reminder_message(tab_id, attempt_id))
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub(crate) fn publish_worker_lifecycle_report(
        &self,
        parent_tab_id: &str,
        worker_tab_id: &str,
        worker_title: &str,
        plan_id: &str,
        task_id: &str,
        payload: Value,
    ) {
        let attempt_id = payload.get("attempt_id").and_then(Value::as_str);
        let tab_provider = {
            self.mux
                .read()
                .get_tab(worker_tab_id)
                .and_then(|tab| tab.agent_type.clone())
        };
        let provider = tab_provider.or_else(|| {
            let plan = self.plan_store.get(plan_id)?;
            let task = crate::plan::task_key::find_task(&plan.tasks, task_id)?;
            task.execution_attempts
                .iter()
                .find(|attempt| attempt_id.is_some_and(|id| attempt.id == id))
                .and_then(|attempt| attempt.agent_type.clone().or(attempt.provider.clone()))
        });
        let provider_output =
            if payload.get("kind").and_then(Value::as_str) == Some("worker_ack_startup_failed") {
                payload
                    .get("attempt_id")
                    .and_then(Value::as_str)
                    .and_then(|attempt_id| self.startup_provider_output(attempt_id))
            } else {
                None
            };
        let message = worker_lifecycle_notice(
            worker_title,
            provider.as_deref(),
            provider_output.as_deref(),
            &payload,
        );
        let details = ReportDetails {
            plan_id: Some(plan_id.to_string()),
            task_id: Some(task_id.to_string()),
            ..Default::default()
        };
        let report = self.report_store.lock().add_detailed(
            parent_tab_id,
            worker_tab_id,
            worker_title,
            &message,
            details,
            None,
        );
        if !self.report_store.lock().claim(&report.id) {
            return;
        }
        self.events.broadcast(
            "agent.report",
            json!({
                "target_tab_id": parent_tab_id,
                "report": report.clone(),
            }),
        );
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "agent-report",
            json!({
                "target_tab_id": parent_tab_id,
                "from_tab_id": worker_tab_id,
                "from_title": worker_title,
                "message": message,
                "report": report.clone(),
            }),
        );
        let envelope =
            crate::service::messenger::MessageEnvelope::report(worker_title, worker_tab_id);
        if let Err(error) = self.gateway_deliver(
            parent_tab_id,
            envelope,
            &report.inject_body(),
            crate::service::message_gateway::DeliveryClass::AgentTraffic,
        ) {
            self.report_store.lock().release_delivery(&report.id);
            log::warn!(
                "[worker-lifecycle] parent delivery failed task={} tab={}: {}",
                task_id,
                worker_tab_id,
                error
            );
        } else {
            self.report_store.lock().mark_delivered(&report.id);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn arm_worker_ack_deadline(
        self: &Arc<Self>,
        parent_tab_id: &str,
        worker_tab_id: &str,
        worker_title: &str,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        spawn_command: String,
    ) {
        let acknowledged = self.plan_store.get(plan_id).is_some_and(|plan| {
            crate::plan::task_key::find_task(&plan.tasks, task_id)
                .into_iter()
                .flat_map(|task| task.execution_attempts.iter())
                .any(|attempt| attempt.id == attempt_id && attempt.acknowledged_at.is_some())
        });
        if acknowledged {
            return;
        }
        let binding = self.mux.read().get_tab(worker_tab_id).map(|tab| {
            (
                tab.agent_session_id.clone(),
                tab.session_binding_nonce.clone(),
            )
        });
        let identity = WorkerAckIdentity {
            plan_id: plan_id.to_string(),
            task_id: task_id.to_string(),
            attempt_id: attempt_id.to_string(),
            tab_id: worker_tab_id.to_string(),
            session_id: binding
                .as_ref()
                .and_then(|(session_id, _)| session_id.clone()),
            binding_nonce: binding.and_then(|(_, nonce)| nonce),
        };
        let receiver = match self.worker_ack.arm(identity.clone()) {
            WorkerAckArm::Armed(receiver) => receiver,
            WorkerAckArm::Extended | WorkerAckArm::AlreadyAcknowledged => return,
        };
        let service = Arc::clone(self);
        let parent_tab_id = parent_tab_id.to_string();
        let worker_tab_id = worker_tab_id.to_string();
        let worker_title = worker_title.to_string();
        let plan_id = plan_id.to_string();
        let task_id = task_id.to_string();
        let attempt_id = attempt_id.to_string();
        std::thread::spawn(move || {
            let mut generation = 0;
            match await_worker_ack_notice(&receiver, WORKER_ACK_TIMEOUT, &mut generation) {
                WorkerAckNoticeOutcome::Acknowledged | WorkerAckNoticeOutcome::Superseded => return,
                WorkerAckNoticeOutcome::Overdue => {}
            }
            if !service.live_unacknowledged_attempt_owns_tab(
                &plan_id,
                &task_id,
                &worker_tab_id,
                &attempt_id,
                None,
            ) {
                return;
            }
            let authenticated = service.authenticated_worker_binding(&worker_tab_id);
            let reminder_allowed = authenticated.as_ref().is_some_and(|(session_id, _)| {
                service.live_unacknowledged_attempt_owns_tab(
                    &plan_id,
                    &task_id,
                    &worker_tab_id,
                    &attempt_id,
                    Some(session_id),
                )
            });
            service.nudge_unacknowledged_startup(&worker_tab_id, &attempt_id);
            if !reminder_allowed {
                let _ = service.plan_store.mark_worker_startup_deadline(
                    &plan_id,
                    &task_id,
                    &worker_tab_id,
                    &attempt_id,
                    false,
                    epoch_ms(),
                );
            }
            loop {
                match await_worker_ack_notice(&receiver, WORKER_ACK_FINAL_TIMEOUT, &mut generation)
                {
                    WorkerAckNoticeOutcome::Acknowledged | WorkerAckNoticeOutcome::Superseded => {
                        return
                    }
                    WorkerAckNoticeOutcome::Overdue => {}
                }
                match service.worker_ack.expire(&identity, generation) {
                    WaiterExpiry::Expired => break,
                    WaiterExpiry::Extended => continue,
                    WaiterExpiry::NoLongerCurrent => return,
                }
            }
            let finalized = service
                .plan_store
                .mark_worker_startup_deadline(
                    &plan_id,
                    &task_id,
                    &worker_tab_id,
                    &attempt_id,
                    true,
                    epoch_ms(),
                )
                .unwrap_or(false);
            if !finalized {
                return;
            }
            service.nudge_unacknowledged_startup(&worker_tab_id, &attempt_id);
            service.publish_worker_lifecycle_report(
                &parent_tab_id,
                &worker_tab_id,
                &worker_title,
                &plan_id,
                &task_id,
                worker_ack_timeout_event(
                    &plan_id,
                    &task_id,
                    &worker_tab_id,
                    &attempt_id,
                    &spawn_command,
                ),
            );
        });
    }

    pub(crate) fn arm_bound_worker_ack_deadline_for_tab(
        self: &Arc<Self>,
        tab_id: &str,
        spawn_command: String,
    ) -> Result<bool, String> {
        use crate::service::plan::{ambiguous_subject_cause, unresolved_subject_cause};
        let bound = self.plan_store.live_task_attempts_for_tab(tab_id);
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
