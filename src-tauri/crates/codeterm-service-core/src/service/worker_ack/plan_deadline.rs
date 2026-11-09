use super::*;
use crate::agent::reports::ReportDetails;
use crate::service::hierarchy::HierarchyReportDelivery;
use crate::service::message_gateway::{DeliveryClass, MessageBody, MessageOrigin, OutboundMessage};
use crate::service::startup_lease::StartupHoldRejection;
use serde_json::json;

#[path = "plan_deadline/task_deadline.rs"]
mod task_deadline;

impl ServiceContext {
    fn startup_provider_output(&self, attempt_id: &str) -> Option<String> {
        self.screen_state
            .get_screen_text(attempt_id)
            .map(|output| {
                crate::service::launch_diagnostics::redact_secrets(
                    &crate::service::launch_diagnostics::tail_bytes(&output, 4096),
                )
            })
            .filter(|output| !output.trim().is_empty())
    }

    pub(super) fn startup_prompt_failure_reason(
        &self,
        tab_id: &str,
        attempt_id: &str,
    ) -> Option<String> {
        let (agent_type, cwd, trust_provenance) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id)?;
            if tab.pty_id.as_deref() != Some(attempt_id) || tab.launch_session_authenticated() {
                return None;
            }
            let agent_type = tab.agent_type.clone()?;
            let trust_provenance =
                match crate::service::directory_trust::directory_trust_provenance(tab) {
                    Ok(()) => "directory-trust provenance is available",
                    Err(crate::service::directory_trust::DirectoryTrustRefusal::NoProvenance) => {
                        "no provenance to auto-answer"
                    }
                    Err(reason) => reason.reason(),
                };
            (agent_type, tab.cwd.clone(), trust_provenance)
        };
        let screen_text = self.screen_state.get_screen_text(attempt_id)?;
        let snapshot = crate::agent::tab_state::TabStateSnapshot {
            tab_id: tab_id.to_string(),
            agent_type: agent_type.clone(),
            session_id: None,
            cwd,
            screen_text: screen_text.clone(),
        };
        self.agent_registry
            .with_provider(&agent_type, |provider| {
                if provider.parse_directory_trust_prompt(&screen_text).is_some() {
                    return Some(format!(
                        "blocked on provider directory-trust prompt; {trust_provenance}; no session binding"
                    ));
                }
                if let Some(classification) = provider.classify_tab_state(&snapshot) {
                    if classification.confidence >= crate::agent::state_reactor::GLOBAL_THRESHOLD {
                        let prompt = match classification.state {
                            crate::agent::tab_state::TabState::WaitingOnAuth => {
                                Some("provider authentication prompt")
                            }
                            crate::agent::tab_state::TabState::UpdatePrompt => {
                                Some("provider update prompt")
                            }
                            crate::agent::tab_state::TabState::RateLimited => {
                                Some("provider rate-limit prompt")
                            }
                            crate::agent::tab_state::TabState::ClarifyingQuestion => {
                                Some("provider question prompt")
                            }
                            _ => None,
                        };
                        if let Some(prompt) = prompt {
                            return Some(format!("blocked on {prompt}; no session binding"));
                        }
                    }
                }
                provider
                    .parse_prompt(&screen_text)
                    .map(|_| "blocked on provider permission prompt; no session binding".into())
            })
            .flatten()
    }

    pub(super) fn agent_startup_failure_payload(
        &self,
        authority: &StartupAuthority,
        agent_title: &str,
        exit_code: Option<i32>,
    ) -> Option<Value> {
        if !self.startup_attempt_is_current(authority) {
            return None;
        }
        let failure_reason = self
            .startup_prompt_failure_reason(&authority.child_tab_id, &authority.attempt_id)
            .filter(|reason| self.record_startup_failure_reason(authority, reason));
        if let Some(reason) = failure_reason.as_deref() {
            log::warn!(
                "[startup-supervision] Tab {} startup deadline failed: {reason}",
                authority.child_tab_id
            );
        }
        let provider_output = self.startup_provider_output(&authority.attempt_id);
        Some(agent_startup_failed_event(
            &authority.child_tab_id,
            agent_title,
            &authority.attempt_id,
            exit_code,
            provider_output.as_deref(),
            failure_reason.as_deref(),
        ))
    }

    pub fn register_agent_startup_supervision(
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
            None,
        );
    }

    /// Register the two launch attempts a pair mint owns together with the lease
    /// on the Team it minted for them.
    pub fn register_pair_startup_lease(
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
                None,
            );
        }
        self.worker_ack.register_startup_lease(lease);
    }

    pub fn cancel_agent_startup_supervision(&self, agent_tab_id: &str) {
        self.worker_ack.cancel_startup_supervisor(agent_tab_id);
    }

    #[cfg(test)]
    pub(crate) fn has_agent_startup_supervision(&self, agent_tab_id: &str) -> bool {
        self.worker_ack.startup_supervisor(agent_tab_id).is_some()
    }

    /// Retire the lease itself. The caller has already settled the Team, so no
    /// later sweep may act on this reservation again.
    pub fn forget_pair_startup_lease(&self, team_id: &str) {
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

    pub fn prepare_agent_startup_supervision(
        &self,
        agent_tab_id: &str,
        expected_supervisor: Option<&str>,
        task_identity: Option<StartupTaskIdentity>,
    ) -> bool {
        if self
            .worker_ack
            .startup_supervisor(agent_tab_id)
            .is_some_and(|existing| {
                existing.task_identity == task_identity
                    && expected_supervisor
                        .is_none_or(|supervisor| existing.supervisor_tab_id == supervisor)
            })
        {
            return true;
        }
        let supervisor = {
            let mux = self.mux.read();
            let Some(agent) = mux.get_tab(agent_tab_id) else {
                return false;
            };
            let Some(supervisor_tab_id) = expected_supervisor
                .or(agent.spawned_by_tab_id.as_deref())
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
                agent.team_id.clone(),
            )
        };
        self.worker_ack.register_startup_supervisor(
            agent_tab_id,
            &supervisor.0,
            &supervisor.1,
            supervisor.2.as_deref(),
            task_identity,
        );
        true
    }

    pub fn activate_agent_startup_supervision(
        &self,
        agent_tab_id: &str,
        attempt_id: &str,
        binding_nonce: &str,
        expected_task_identity: Option<&StartupTaskIdentity>,
    ) -> Result<(), String> {
        let (session_id, task_binding, remote_task_bound) = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(agent_tab_id) else {
                return Err(format!("task-bound startup tab {agent_tab_id} disappeared"));
            };
            if tab.pty_id.as_deref() != Some(attempt_id)
                || tab.session_binding_nonce.as_deref() != Some(binding_nonce)
            {
                return Err(format!(
                    "startup waiter binding for Tab {agent_tab_id} does not own PTY {attempt_id}"
                ));
            }
            let task_binding = tab.task_binding.clone();
            let remote_task_bound = expected_task_identity.is_some_and(|identity| {
                tab.report_target_mesh_peer
                    .as_ref()
                    .is_some_and(|peer| !peer.is_empty())
                    && task_binding.as_ref().is_some_and(|binding| {
                        binding.plan_id.as_deref() == Some(identity.plan_id.as_str())
                            && binding.task_id.as_deref() == Some(identity.task_id.as_str())
                            && binding.attempt_id.as_deref() == Some(identity.attempt_id.as_str())
                    })
            });
            (
                tab.agent_session_id.clone(),
                task_binding,
                remote_task_bound,
            )
        };
        if remote_task_bound {
            return Ok(());
        }
        let task_bound = expected_task_identity.is_some()
            || task_binding
                .as_ref()
                .is_some_and(|binding| binding.plan_id.is_some() && binding.task_id.is_some());
        let Some(supervisor) = self.worker_ack.startup_supervisor(agent_tab_id) else {
            return if task_bound {
                Err(format!(
                    "task-bound dispatch for Tab {agent_tab_id} has no supervised startup waiter"
                ))
            } else {
                Ok(())
            };
        };
        if task_bound {
            let Some(identity) = supervisor.task_identity.as_ref() else {
                return Err(format!(
                    "task-bound dispatch for Tab {agent_tab_id} has no task attempt in its startup waiter"
                ));
            };
            if expected_task_identity.is_some_and(|expected| expected != identity) {
                return Err(format!(
                    "startup waiter task attempt {}/{} does not match dispatched task attempt {}/{}",
                    identity.task_id,
                    identity.attempt_id,
                    expected_task_identity
                        .map(|expected| expected.task_id.as_str())
                        .unwrap_or_default(),
                    expected_task_identity
                        .map(|expected| expected.attempt_id.as_str())
                        .unwrap_or_default()
                ));
            }
            if let Some(binding) = task_binding.as_ref() {
                if binding
                    .plan_id
                    .as_deref()
                    .is_some_and(|plan_id| plan_id != identity.plan_id.as_str())
                    || binding
                        .task_id
                        .as_deref()
                        .is_some_and(|task_id| task_id != identity.task_id.as_str())
                {
                    return Err(format!(
                        "startup waiter task {}/{} does not match Tab {agent_tab_id} task binding",
                        identity.plan_id, identity.task_id
                    ));
                }
            }
        }
        let owner_epoch = match supervisor.task_identity.as_ref() {
            Some(identity) => self
                .plan_store
                .startup_owner_epoch_projection(
                    &identity.plan_id,
                    &identity.task_id,
                    &identity.attempt_id,
                    agent_tab_id,
                )
                .map_err(|error| error.to_string())?,
            None => None,
        };
        self.arm_agent_startup_deadline(
            StartupAuthority::new(&supervisor.supervisor_tab_id, agent_tab_id, attempt_id)
                .in_team(supervisor.team_id.clone())
                .for_task(supervisor.task_identity.clone())
                .for_task_owner_epoch(owner_epoch)
                .for_binding(session_id.as_deref(), binding_nonce),
            &supervisor.agent_title,
        );
        Ok(())
    }

    pub fn child_startup_state(&self, tab_id: &str) -> Option<ChildStartupState> {
        self.mux
            .read()
            .get_tab(tab_id)
            .map(|tab| ChildStartupState {
                attempt_id: tab.pty_id.clone(),
                team_id: tab.team_id.clone(),
                session_id: tab.agent_session_id.clone(),
                binding_nonce: tab.session_binding_nonce.clone(),
            })
    }

    fn startup_attempt_is_current(&self, authority: &StartupAuthority) -> bool {
        self.mux
            .read()
            .get_tab(&authority.child_tab_id)
            .is_some_and(|tab| {
                tab.pty_id.as_deref() == Some(authority.attempt_id.as_str())
                    && tab.agent_session_id == authority.session_id
                    && tab.session_binding_nonce == authority.binding_nonce
            })
    }

    pub(super) fn startup_attempt_binding_observed(&self, authority: &StartupAuthority) -> bool {
        self.mux
            .read()
            .get_tab(&authority.child_tab_id)
            .is_some_and(|tab| {
                tab.pty_id.as_deref() == Some(authority.attempt_id.as_str())
                    && tab.session_binding_nonce == authority.binding_nonce
                    && authority
                        .session_id
                        .as_ref()
                        .is_none_or(|session| tab.agent_session_id.as_ref() == Some(session))
                    && tab.launch_session_authenticated()
            })
    }

    pub(crate) fn publish_agent_startup_report(
        &self,
        supervisor_tab_id: &str,
        agent_tab_id: &str,
        agent_title: &str,
        payload: Value,
    ) {
        self.publish_agent_startup_report_with_delivery(
            supervisor_tab_id,
            agent_tab_id,
            agent_title,
            payload,
            self,
        );
    }

    pub(crate) fn publish_agent_startup_report_with_delivery(
        &self,
        supervisor_tab_id: &str,
        agent_tab_id: &str,
        agent_title: &str,
        payload: Value,
        delivery: &impl HierarchyReportDelivery,
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
        let gate = self.hierarchy_report_recipient_gate(supervisor_tab_id);
        self.events.broadcast(
            "agent.report",
            json!({ "target_tab_id": supervisor_tab_id, "report": report.clone() }),
        );
        self.ui.emit(
            "agent-report",
            serde_json::to_value(json!({
                "target_tab_id": supervisor_tab_id,
                "from_tab_id": agent_tab_id,
                "from_title": agent_title,
                "message": message,
                "report": report.clone(),
            }))
            .unwrap_or_default(),
        );
        self.deliver_hierarchy_report_if_allowed(&gate, &report, delivery);
    }

    pub fn acknowledge_agent_startup(
        &self,
        caller_tab_id: &str,
        attempt_id: &str,
        binding_nonce: &str,
    ) -> Result<AgentStartupAcknowledgement, String> {
        let task_binding = {
            let mux = self.mux.read();
            mux.get_tab(caller_tab_id)
                .and_then(|tab| tab.task_binding.as_ref())
                .is_some_and(|binding| binding.plan_id.is_some() && binding.task_id.is_some())
        };
        let supervised_task = self
            .worker_ack
            .startup_supervisor(caller_tab_id)
            .is_some_and(|supervisor| supervisor.task_identity.is_some());
        if task_binding || supervised_task {
            return Err("task-bound startup must be acknowledged with `domios agent ack`".into());
        }
        let session_id = self.mux.read().get_tab(caller_tab_id).and_then(|tab| {
            startup_ack_owns_attempt(tab, attempt_id, binding_nonce)
                .then(|| tab.agent_session_id.clone())
        });
        let Some(session_id) = session_id else {
            return Err(format!(
                "startup acknowledgement does not own attempt {attempt_id} for Tab {caller_tab_id}"
            ));
        };
        self.worker_ack.resolve_startup_ack_for_binding(
            caller_tab_id,
            attempt_id,
            session_id.as_deref(),
            binding_nonce,
        )
    }

    /// A runtime-hosted launch has no PTY attempt to carry, so its ACK is honest
    /// only while nothing is owed; any owed handshake still needs its attempt.
    pub fn acknowledge_attemptless_agent_startup(
        &self,
        caller_tab_id: &str,
        binding_nonce: &str,
    ) -> Result<AgentStartupAcknowledgement, String> {
        let (task_bound, attemptless) = {
            let mux = self.mux.read();
            let tab = mux
                .get_tab(caller_tab_id)
                .filter(|tab| tab.session_binding_nonce.as_deref() == Some(binding_nonce))
                .ok_or_else(|| {
                    format!("startup acknowledgement does not own Tab {caller_tab_id}")
                })?;
            (
                tab.task_binding
                    .as_ref()
                    .is_some_and(|binding| binding.plan_id.is_some() && binding.task_id.is_some()),
                tab.pty_id.is_none(),
            )
        };
        if task_bound {
            return Err("task-bound startup must be acknowledged with `domios agent ack`".into());
        }
        if !attemptless || self.worker_ack.startup_handshake_owed(caller_tab_id) {
            return Err(format!(
                "startup acknowledgement for Tab {caller_tab_id} requires its launch attempt; CODETERM_STARTUP_ATTEMPT_ID is not set"
            ));
        }
        Ok(AgentStartupAcknowledgement::NotRequired)
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
    pub fn hold_agent_startup(
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
                if let Err(error) = self.extend_worker_ack_deadline_for_tab(tab_id) {
                    log::warn!(
                        "[worker-ack] held startup ACK extension read failed tab={tab_id}: {error}"
                    );
                }
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
        let receiver = match self.worker_ack.arm_startup(authority.clone()) {
            AgentStartupArm::Armed(receiver) => receiver,
            AgentStartupArm::Extended => return,
            AgentStartupArm::AlreadyAcknowledged => {
                self.worker_ack.clear_startup_supervisor(&authority);
                if let Some(team_id) = authority.team_id.as_deref() {
                    self.worker_ack.release_startup_lease(team_id);
                }
                return;
            }
        };
        let supervisor_tab_id = authority.creator_tab_id.clone();
        let agent_tab_id = authority.child_tab_id.clone();
        let attempt_id = authority.attempt_id.clone();
        let service = self.clone();
        let agent_title = agent_title.to_string();
        std::thread::spawn(move || {
            let mut generation = 0;
            let mut exit_code = match await_agent_startup_notice_with_generation(
                &receiver,
                AGENT_STARTUP_NOTICE_TIMEOUT,
                &mut generation,
            ) {
                AgentStartupNoticeOutcome::Resolved => return,
                AgentStartupNoticeOutcome::Overdue { exit_code } => exit_code,
            };
            if !service.startup_attempt_is_current(&authority)
                || service.startup_attempt_binding_observed(&authority)
            {
                return;
            }
            let mut notice = agent_startup_notice_event(&agent_tab_id, &agent_title, &attempt_id);
            let waiting_for_owner_prompt =
                crate::service::launch_readiness::owner_prompt_is_active(&agent_tab_id);
            if waiting_for_owner_prompt {
                notice["waiting_for_owner_prompt"] = serde_json::Value::Bool(true);
            }
            if !service.startup_attempt_is_current(&authority)
                || service.startup_attempt_binding_observed(&authority)
            {
                return;
            }
            service.publish_agent_startup_report(
                &supervisor_tab_id,
                &agent_tab_id,
                &agent_title,
                notice,
            );
            if !service.startup_attempt_is_current(&authority) {
                return;
            }
            if waiting_for_owner_prompt {
                if crate::service::launch_readiness::await_owner_prompt_resolution(&agent_tab_id)
                    .is_some()
                {
                    return;
                }
                if !service.startup_attempt_is_current(&authority) {
                    return;
                }
            }
            service.nudge_unacknowledged_startup(&agent_tab_id, &attempt_id);
            loop {
                match await_agent_startup_notice_with_generation(
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
                match service
                    .worker_ack
                    .expire_startup_generation(&authority, generation)
                {
                    WaiterExpiry::Expired => break,
                    WaiterExpiry::Extended => continue,
                    WaiterExpiry::NoLongerCurrent => return,
                }
            }
            if let Some(payload) =
                service.agent_startup_failure_payload(&authority, &agent_title, exit_code)
            {
                service.publish_agent_startup_report(
                    &supervisor_tab_id,
                    &agent_tab_id,
                    &agent_title,
                    payload,
                );
            }
        });
    }

    fn live_unacknowledged_attempt_owns_tab(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
        session_id: Option<&str>,
    ) -> Result<bool, crate::service::plan::PlanError> {
        self.plan_store
            .worker_startup_attempt_owns(plan_id, task_id, tab_id, attempt_id, session_id)
    }

    fn authenticated_worker_binding(&self, tab_id: &str) -> Option<(String, String)> {
        self.mux.read().get_tab(tab_id).and_then(|tab| {
            super::super::task_assignment::authenticated_tab_binding(tab)
                .map(|binding| (binding.session_id, binding.session_binding_nonce))
        })
    }

    fn worker_ack_identity_for_attempt(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt_id: &str,
    ) -> WorkerAckIdentity {
        let binding = self.mux.read().get_tab(tab_id).map(|tab| {
            (
                tab.pty_id.clone(),
                tab.agent_session_id.clone(),
                tab.session_binding_nonce.clone(),
            )
        });
        let (startup_attempt_id, session_id, binding_nonce) = binding.unwrap_or_default();
        WorkerAckIdentity {
            plan_id: plan_id.to_string(),
            task_id: task_id.to_string(),
            attempt_id: attempt_id.to_string(),
            tab_id: tab_id.to_string(),
            startup_attempt_id,
            session_id,
            binding_nonce,
        }
    }

    fn worker_ack_identity_is_current(
        &self,
        identity: &WorkerAckIdentity,
    ) -> Result<bool, crate::service::plan::PlanError> {
        let tab_binding = self.mux.read().get_tab(&identity.tab_id).map(|tab| {
            (
                tab.pty_id.clone(),
                tab.agent_session_id.clone(),
                tab.session_binding_nonce.clone(),
            )
        });
        if !tab_binding.is_some_and(|binding| {
            binding.0 == identity.startup_attempt_id
                && binding.1 == identity.session_id
                && binding.2 == identity.binding_nonce
        }) {
            return Ok(false);
        }
        self.plan_store.worker_startup_attempt_owns(
            &identity.plan_id,
            &identity.task_id,
            &identity.tab_id,
            &identity.attempt_id,
            identity.session_id.as_deref(),
        )
    }

    pub fn extend_worker_ack_deadline_for_tab(&self, tab_id: &str) -> Result<bool, String> {
        let armable = self
            .plan_store
            .live_task_attempts_for_tab(tab_id)
            .map_err(|error| error.to_string())?
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
            return Ok(false);
        };
        let identity = self.worker_ack_identity_for_attempt(
            &candidate.plan_id,
            &candidate.task_id,
            tab_id,
            &candidate.attempt.id,
        );
        Ok(self.worker_ack.extend_deadline(&identity))
    }

    pub fn acknowledge_worker_ack(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        attempt: &crate::plan::TaskExecutionAttempt,
    ) -> bool {
        let identity = self.worker_ack_identity_for_attempt(plan_id, task_id, tab_id, &attempt.id);
        let acknowledged = self.worker_ack.acknowledge(&identity);
        self.worker_ack.clear_ack_evidence(tab_id);
        let task_identity = StartupTaskIdentity {
            plan_id: plan_id.to_string(),
            task_id: task_id.to_string(),
            attempt_id: attempt.id.clone(),
        };
        self.worker_ack
            .acknowledge_task_startup(tab_id, &task_identity, &identity);
        self.schedule_session_model_refresh_for_current_tab(tab_id);
        acknowledged
    }

    /// Nudge a tab that has not acknowledged its startup. The tab's own state
    /// decides whether the nudge is this text at all: a terminal still owned by
    /// a provider dialog gets that dialog answered instead, and a terminal with
    /// no agent behind it gets nothing.
    pub(crate) fn nudge_unacknowledged_startup(&self, tab_id: &str, attempt_id: &str) {
        let structural = self.classify_startup_nudge(tab_id);
        self.shadow_tab_state(tab_id, structural);
        if !structural.delivers() {
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
        let provider = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.agent_type.clone());
        if provider
            .and_then(|provider| {
                self.agent_registry
                    .with_provider(&provider, |provider| provider.is_tool_less())
            })
            .unwrap_or(false)
        {
            return Ok(());
        }
        self.gateway
            .submit(self, worker_ack_reminder_message(tab_id, attempt_id))
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub fn publish_worker_lifecycle_report(
        &self,
        parent_tab_id: &str,
        worker_tab_id: &str,
        worker_title: &str,
        plan_id: &str,
        task_id: &str,
        payload: Value,
    ) {
        self.publish_worker_lifecycle_report_with_delivery(
            parent_tab_id,
            worker_tab_id,
            worker_title,
            plan_id,
            task_id,
            payload,
            self,
        );
    }

    pub(crate) fn publish_worker_lifecycle_report_with_delivery(
        &self,
        parent_tab_id: &str,
        worker_tab_id: &str,
        worker_title: &str,
        plan_id: &str,
        task_id: &str,
        payload: Value,
        delivery: &impl HierarchyReportDelivery,
    ) {
        if payload.get("kind").and_then(Value::as_str) == Some("tab_state_spawn_ok")
            && !crate::service::supporter_notify::spawn_ok_notice_allowed(
                crate::service::supporter_notify::launch_was_restored(parent_tab_id),
            )
        {
            return;
        }
        let attempt_id = payload.get("attempt_id").and_then(Value::as_str);
        let tab_provider = {
            self.mux
                .read()
                .get_tab(worker_tab_id)
                .and_then(|tab| tab.agent_type.clone())
        };
        let provider = match (tab_provider, attempt_id) {
            (Some(provider), _) => Some(provider),
            (None, Some(attempt)) => match self
                .plan_store
                .lifecycle_provider_projection(plan_id, task_id, attempt)
            {
                Ok(provider) => provider,
                Err(error) => {
                    log::warn!("[worker-lifecycle] provider read failed plan={plan_id} task={task_id} attempt={attempt}: {error}");
                    return;
                }
            },
            (None, None) => None,
        };
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
        let gate = self.hierarchy_report_recipient_gate(parent_tab_id);
        self.events.broadcast(
            "agent.report",
            json!({
                "target_tab_id": parent_tab_id,
                "report": report.clone(),
            }),
        );
        self.ui.emit(
            "agent-report",
            serde_json::to_value(json!({
                "target_tab_id": parent_tab_id,
                "from_tab_id": worker_tab_id,
                "from_title": worker_title,
                "message": message,
                "report": report.clone(),
            }))
            .unwrap_or_default(),
        );
        self.deliver_hierarchy_report_if_allowed(&gate, &report, delivery);
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
