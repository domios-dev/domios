use super::*;
use crate::agent::reports::ReportDetails;
use crate::service::message_gateway::{DeliveryClass, MessageBody, MessageOrigin, OutboundMessage};
use crate::service::startup_lease::StartupHoldRejection;
use serde_json::json;
use std::sync::Arc;

impl ServiceContext {
    fn startup_provider_output(&self, attempt_id: &str) -> Option<String> {
        self.screen_state
            .lock()
            .get_screen_text(attempt_id)
            .map(|output| {
                crate::service::launch_diagnostics::redact_secrets(
                    &crate::service::launch_diagnostics::tail_bytes(&output, 4096),
                )
            })
            .filter(|output| !output.trim().is_empty())
    }

    pub(crate) fn register_agent_startup_supervision(
        &self,
        supervisor_tab_id: &str,
        agent_tab_id: &str,
        agent_title: &str,
    ) {
        self.worker_ack.register_startup_supervisor(
            agent_tab_id,
            supervisor_tab_id,
            agent_title,
            None,
        );
    }

    /// Register the two launch attempts a pair mint owns together with the lease
    /// on the Team it minted for them.
    pub(crate) fn register_pair_startup_lease(
        &self,
        lease: StartupTeamLease,
        titled_members: &[(&str, &str)],
    ) {
        for (agent_tab_id, agent_title) in titled_members.iter().copied() {
            self.worker_ack.register_startup_supervisor(
                agent_tab_id,
                &lease.creator_tab_id,
                agent_title,
                Some(&lease.team_id),
            );
        }
        self.worker_ack.register_startup_lease(lease);
    }

    pub(crate) fn cancel_agent_startup_supervision(&self, agent_tab_id: &str) {
        self.worker_ack.cancel_startup_supervisor(agent_tab_id);
    }

    /// Retire the lease itself. The caller has already settled the Team, so no
    /// later sweep may act on this reservation again.
    pub(crate) fn forget_pair_startup_lease(&self, team_id: &str) {
        self.worker_ack.release_startup_lease(team_id);
    }

    /// Reaps every lifecycle-owned forming Team whose reserved members are gone.
    /// Mux locks first, lease map inside: no lease-map holder may reach for mux.
    pub(crate) fn reap_orphaned_startup_teams(&self) -> Vec<String> {
        let reaped = {
            let mut mux = self.mux.write();
            self.worker_ack.reap_startup_leases(&mut mux)
        };
        for team_id in &reaped {
            log::info!("[startup-lease] reaped empty forming pair Team {team_id}");
        }
        reaped
    }

    pub(crate) fn prepare_agent_startup_supervision(&self, agent_tab_id: &str) -> bool {
        if self.worker_ack.has_startup_supervisor(agent_tab_id) {
            return true;
        }
        let supervisor = {
            let mux = self.mux.read();
            let Some(agent) = mux.get_tab(agent_tab_id) else {
                return false;
            };
            let Some(supervisor_tab_id) = agent
                .spawned_by_tab_id
                .as_deref()
                .or(agent.parent_tab_id.as_deref())
            else {
                return false;
            };
            let Some(supervisor) = mux.get_tab(supervisor_tab_id) else {
                return false;
            };
            if !is_agent_spawner(supervisor) {
                return false;
            }
            (
                supervisor_tab_id.to_string(),
                mux.display_label_for_tab(agent_tab_id),
            )
        };
        self.register_agent_startup_supervision(&supervisor.0, agent_tab_id, &supervisor.1);
        true
    }

    pub(crate) fn activate_agent_startup_supervision(&self, agent_tab_id: &str, attempt_id: &str) {
        let Some(supervisor) = self.worker_ack.take_startup_supervisor(agent_tab_id) else {
            return;
        };
        self.arm_agent_startup_deadline(
            StartupAuthority::new(&supervisor.supervisor_tab_id, agent_tab_id, attempt_id)
                .in_team(supervisor.team_id.clone()),
            &supervisor.agent_title,
        );
    }

    fn child_startup_state(&self, tab_id: &str) -> Option<ChildStartupState> {
        self.mux
            .read()
            .get_tab(tab_id)
            .map(|tab| ChildStartupState {
                attempt_id: tab.pty_id.clone(),
                team_id: tab.team_id.clone(),
            })
    }

    fn startup_attempt_is_current(&self, tab_id: &str, attempt_id: &str) -> bool {
        self.mux
            .read()
            .get_tab(tab_id)
            .is_some_and(|tab| tab.pty_id.as_deref() == Some(attempt_id))
    }

    fn publish_agent_startup_report(
        &self,
        supervisor_tab_id: &str,
        agent_tab_id: &str,
        agent_title: &str,
        payload: Value,
    ) {
        let provider = self
            .mux
            .read()
            .get_tab(agent_tab_id)
            .and_then(|tab| tab.agent_type.clone());
        let message = worker_lifecycle_notice(agent_title, provider.as_deref(), None, &payload);
        let report = self.report_store.lock().add_detailed(
            supervisor_tab_id,
            agent_tab_id,
            agent_title,
            &message,
            ReportDetails::default(),
            None,
        );
        if !self.report_store.lock().claim(&report.id) {
            return;
        }
        self.events.broadcast(
            "agent.report",
            json!({ "target_tab_id": supervisor_tab_id, "report": report.clone() }),
        );
        crate::api::events::emit_everywhere(
            &self.app_handle,
            "agent-report",
            json!({
                "target_tab_id": supervisor_tab_id,
                "from_tab_id": agent_tab_id,
                "from_title": agent_title,
                "message": message,
                "report": report.clone(),
            }),
        );
        let envelope =
            crate::service::messenger::MessageEnvelope::report(agent_title, agent_tab_id);
        if self
            .gateway_deliver(
                supervisor_tab_id,
                envelope,
                &report.inject_body(),
                crate::service::message_gateway::DeliveryClass::AgentTraffic,
            )
            .is_ok()
        {
            self.report_store.lock().mark_delivered(&report.id);
        } else {
            self.report_store.lock().release_delivery(&report.id);
        }
    }

    pub(crate) fn acknowledge_agent_startup(
        &self,
        caller_tab_id: &str,
        attempt_id: &str,
        binding_nonce: &str,
    ) -> Result<AgentStartupAcknowledgement, String> {
        let authorized = self
            .mux
            .read()
            .get_tab(caller_tab_id)
            .is_some_and(|tab| startup_ack_owns_attempt(tab, attempt_id, binding_nonce));
        if !authorized {
            return Err(format!(
                "startup acknowledgement does not own attempt {attempt_id} for Tab {caller_tab_id}"
            ));
        }
        self.worker_ack
            .acknowledge_startup(caller_tab_id, attempt_id)
            .ok_or_else(|| {
                format!(
                    "startup attempt {attempt_id} for Tab {caller_tab_id} is not awaiting acknowledgement"
                )
            })
    }

    pub(crate) fn observe_agent_startup_launch_command_finished(
        &self,
        tab_id: &str,
        attempt_id: &str,
        exit_code: Option<i32>,
    ) {
        self.worker_ack
            .mark_startup_launch_command_finished(tab_id, attempt_id, exit_code);
    }

    /// Only the recorded creator of this exact child attempt may hold it. The
    /// accepted and refused shapes both carry auditable provenance.
    pub(crate) fn hold_agent_startup(
        &self,
        caller_tab_id: &str,
        tab_id: &str,
        attempt_id: &str,
    ) -> Result<Value, StartupHoldRejection> {
        let child = self.child_startup_state(tab_id);
        match self.worker_ack.hold_startup_authorized(
            caller_tab_id,
            tab_id,
            attempt_id,
            child.as_ref(),
        ) {
            Ok(authority) => {
                self.extend_worker_ack_deadline_for_tab(tab_id);
                Ok(json!({
                    "ok": true,
                    "held": true,
                    "state": "held_by_creator",
                    "tab_id": authority.child_tab_id,
                    "attempt_id": authority.attempt_id,
                    "authority": authority.provenance(),
                }))
            }
            Err((refusal, supervised)) => Err(StartupHoldRejection {
                message: refusal.message(caller_tab_id, tab_id, attempt_id),
                provenance: refusal.provenance(
                    caller_tab_id,
                    tab_id,
                    attempt_id,
                    supervised.as_ref(),
                ),
            }),
        }
    }

    pub(crate) fn arm_agent_startup_deadline(
        &self,
        authority: StartupAuthority,
        agent_title: &str,
    ) {
        let supervisor_tab_id = authority.creator_tab_id.clone();
        let agent_tab_id = authority.child_tab_id.clone();
        let attempt_id = authority.attempt_id.clone();
        let receiver = self.worker_ack.arm_startup(authority);
        let service = self.clone();
        let agent_title = agent_title.to_string();
        std::thread::spawn(move || {
            let final_deadline = std::time::Instant::now()
                + AGENT_STARTUP_NOTICE_TIMEOUT
                + AGENT_STARTUP_FINAL_TIMEOUT;
            let mut exit_code =
                match await_agent_startup_notice(&receiver, AGENT_STARTUP_NOTICE_TIMEOUT) {
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
                let remaining = final_deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                match receiver.recv_timeout(remaining) {
                    Ok(
                        AgentStartupSignal::Ready
                        | AgentStartupSignal::Held
                        | AgentStartupSignal::Superseded,
                    ) => return,
                    Ok(AgentStartupSignal::LaunchCommandFinished(code)) => exit_code = code,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                }
            }
            let Some(_expired) = service
                .worker_ack
                .expire_startup(&agent_tab_id, &attempt_id)
            else {
                return;
            };
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

    fn worker_ack_identity_for_attempt(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt: &crate::plan::TaskExecutionAttempt,
    ) -> WorkerAckIdentity {
        let binding = self.mux.read().get_tab(tab_id).map(|tab| {
            (
                tab.agent_session_id.clone(),
                tab.session_binding_nonce.clone(),
            )
        });
        let (session_id, binding_nonce) = binding.unwrap_or_default();
        WorkerAckIdentity {
            plan_id: plan_id.to_string(),
            task_id: task_id.to_string(),
            attempt_id: attempt.id.clone(),
            tab_id: tab_id.to_string(),
            session_id: session_id.or_else(|| attempt.session_id.clone()),
            binding_nonce: binding_nonce.or_else(|| attempt.session_binding_nonce.clone()),
        }
    }

    fn worker_ack_identity_is_current(&self, identity: &WorkerAckIdentity) -> bool {
        let tab_binding = self.mux.read().get_tab(&identity.tab_id).map(|tab| {
            (
                tab.agent_session_id.clone(),
                tab.session_binding_nonce.clone(),
            )
        });
        tab_binding.is_some_and(|binding| {
            binding.0 == identity.session_id && binding.1 == identity.binding_nonce
        }) && self.plan_store.worker_startup_attempt_owns(
            &identity.plan_id,
            &identity.task_id,
            &identity.tab_id,
            &identity.attempt_id,
            identity.session_id.as_deref(),
        )
    }

    pub(super) fn extend_worker_ack_deadline_for_tab(&self, tab_id: &str) -> bool {
        let armable = self
            .plan_store
            .live_task_attempts_for_tab(tab_id)
            .into_iter()
            .filter(|candidate| {
                candidate.plan_active
                    && matches!(
                        candidate.task_status,
                        crate::plan::TaskStatus::Pending | crate::plan::TaskStatus::InProgress
                    )
                    && candidate.attempt.acknowledged_at.is_none()
                    && candidate.attempt.retired_at.is_none()
            })
            .collect::<Vec<_>>();
        let [candidate] = armable.as_slice() else {
            return false;
        };
        let identity = self.worker_ack_identity_for_attempt(
            &candidate.plan_id,
            &candidate.task_id,
            tab_id,
            &candidate.attempt,
        );
        self.worker_ack.extend_deadline(&identity)
    }

    pub(crate) fn acknowledge_worker_ack(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt: &crate::plan::TaskExecutionAttempt,
    ) -> bool {
        let identity = self.worker_ack_identity_for_attempt(plan_id, task_id, tab_id, attempt);
        self.worker_ack.acknowledge(&identity)
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
        let provider_output = if payload.get("kind").and_then(Value::as_str)
            == Some("worker_ack_startup_failed")
        {
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
        let Some(attempt) = self
            .plan_store
            .get(plan_id)
            .filter(|plan| plan.status == crate::plan::PlanStatus::Active)
            .and_then(|plan| crate::plan::task_key::find_task(&plan.tasks, task_id).cloned())
            .filter(|task| {
                matches!(
                    task.status,
                    crate::plan::TaskStatus::Pending | crate::plan::TaskStatus::InProgress
                )
            })
            .and_then(|task| {
                task.execution_attempts
                    .into_iter()
                    .find(|attempt| {
                        attempt.id == attempt_id
                            && attempt.executor_tab_id == worker_tab_id
                            && attempt.closed_at.is_none()
                            && attempt.retired_at.is_none()
                    })
            })
        else {
            return;
        };
        if attempt.acknowledged_at.is_some() {
            return;
        }
        let identity =
            self.worker_ack_identity_for_attempt(plan_id, task_id, worker_tab_id, &attempt);
        if !self.worker_ack_identity_is_current(&identity) {
            return;
        }
        let arm = self.worker_ack.arm(identity.clone());
        if arm.identity_conflict {
            log::warn!(
                "[worker-ack] conflicting waiter identity task={} attempt={} tab={}",
                task_id,
                attempt_id,
                worker_tab_id
            );
            return;
        }
        if !arm.starts_supervisor {
            return;
        }
        let Some(receiver) = arm.receiver else {
            return;
        };
        let service = Arc::clone(self);
        let parent_tab_id = parent_tab_id.to_string();
        let worker_tab_id = worker_tab_id.to_string();
        let worker_title = worker_title.to_string();
        let plan_id = plan_id.to_string();
        let task_id = task_id.to_string();
        let attempt_id = attempt_id.to_string();
        std::thread::spawn(move || {
            if await_worker_ack(&receiver, WORKER_ACK_TIMEOUT) == WorkerAckWaitOutcome::Resolved {
                return;
            }
            if !service.worker_ack_identity_is_current(&identity) {
                return;
            }
            let authenticated = service.authenticated_worker_binding(&worker_tab_id);
            let reminder_allowed = authenticated.as_ref().is_some_and(|(session_id, nonce)| {
                identity.session_id.as_deref() == Some(session_id)
                    && identity.binding_nonce.as_deref() == Some(nonce)
                    && service.live_unacknowledged_attempt_owns_tab(
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
            if await_worker_ack(&receiver, WORKER_ACK_FINAL_TIMEOUT)
                == WorkerAckWaitOutcome::Resolved
            {
                return;
            }
            if !service.worker_ack.expire(&identity) {
                return;
            }
            if !service.worker_ack_identity_is_current(&identity) {
                return;
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

pub(crate) fn worker_ack_reminder_message(tab_id: &str, attempt_id: &str) -> OutboundMessage {
    let envelope =
        crate::service::messenger::MessageEnvelope::codeterm_reminder(WORKER_ACK_CONTEXT)
            .id(Some(attempt_id.to_string()));
    OutboundMessage {
        target: tab_id.to_string(),
        body: MessageBody::Text {
            body: WORKER_ACK_REMINDER.to_string(),
        },
        envelope: Some(envelope),
        class: DeliveryClass::SystemNotice,
        origin: MessageOrigin::System,
        idempotency_key: Some(format!("worker-ack-reminder:{attempt_id}")),
    }
}

