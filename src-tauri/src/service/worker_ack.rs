use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Value};

use crate::agent::reports::ReportDetails;
use crate::multiplexer::state::MultiplexerState;
use crate::service::message_gateway::{DeliveryClass, MessageBody, MessageOrigin, OutboundMessage};
use crate::service::startup_lease::{
    authorize_startup_hold, ChildStartupState, StartupAuthority, StartupHoldRefusal,
    StartupHoldRejection, StartupTeamLease,
};

use super::ServiceContext;

const MEASURED_CLAUDE_COLD_START_SECS: u64 = 34;
const MEASURED_CODEX_COLD_START_SECS: u64 = 38;
const COLD_START_DEADLINE_SECS: u64 = 120;
const COLD_START_MEASUREMENT_METHOD: &str =
    "visible-spawn launch to first session binding in lifecycle logs";
pub(crate) const TAB_STATE_ALIVE_AWAITING_INSPECTION: &str = "alive_awaiting_inspection";
pub(crate) const ATTEMPT_STATE_FINALIZED: &str = "finalized";
pub(crate) const STARTUP_BINDING_DEADLINE: Duration = Duration::from_secs(COLD_START_DEADLINE_SECS);
const WORKER_ACK_TIMEOUT: Duration = STARTUP_BINDING_DEADLINE;
const WORKER_ACK_FINAL_TIMEOUT: Duration = Duration::from_secs(COLD_START_DEADLINE_SECS);
const AGENT_STARTUP_NOTICE_TIMEOUT: Duration = STARTUP_BINDING_DEADLINE;
const AGENT_STARTUP_FINAL_TIMEOUT: Duration = Duration::from_secs(COLD_START_DEADLINE_SECS);
const WORKER_ACK_REMINDER: &str = "The startup binding for this tab looks wrong. Check whether CODETERM_TASK_ID names a task with no live attempt or whether this session never bound. If the assignment is live, acknowledge it with codeterm agent ack.";
pub(crate) const WORKER_ACK_CONTEXT: &str = "worker-ack";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkerAckSignal {
    Acknowledged,
    Superseded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentStartupSignal {
    Ready,
    Held,
    LaunchCommandFinished(Option<i32>),
    Superseded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentStartupNoticeOutcome {
    Resolved,
    Overdue { exit_code: Option<i32> },
}

fn await_agent_startup_notice(
    receiver: &Receiver<AgentStartupSignal>,
    timeout: Duration,
) -> AgentStartupNoticeOutcome {
    let deadline = std::time::Instant::now() + timeout;
    let mut exit_code = None;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return AgentStartupNoticeOutcome::Overdue { exit_code };
        }
        match receiver.recv_timeout(remaining) {
            Ok(
                AgentStartupSignal::Ready
                | AgentStartupSignal::Held
                | AgentStartupSignal::Superseded,
            )
            | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return AgentStartupNoticeOutcome::Resolved;
            }
            Ok(AgentStartupSignal::LaunchCommandFinished(code)) => exit_code = code,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                return AgentStartupNoticeOutcome::Overdue { exit_code };
            }
        }
    }
}

struct WorkerAckWaiter {
    attempt_id: String,
    sender: Sender<WorkerAckSignal>,
}

struct AgentStartupWaiter {
    authority: StartupAuthority,
    sender: Sender<AgentStartupSignal>,
}

struct PendingAgentStartupSupervisor {
    supervisor_tab_id: String,
    agent_title: String,
    team_id: Option<String>,
}

pub(crate) fn is_agent_spawner(tab: &crate::multiplexer::types::Tab) -> bool {
    tab.agent_type.is_some()
}

fn startup_ack_owns_attempt(
    tab: &crate::multiplexer::types::Tab,
    attempt_id: &str,
    binding_nonce: &str,
) -> bool {
    tab.pty_id.as_deref() == Some(attempt_id)
        && tab.session_binding_nonce.as_deref() == Some(binding_nonce)
}

#[derive(Default)]
pub(crate) struct WorkerAckRegistry {
    waiters: Mutex<HashMap<String, WorkerAckWaiter>>,
    startup_waiters: Mutex<HashMap<String, AgentStartupWaiter>>,
    startup_supervisors: Mutex<HashMap<String, PendingAgentStartupSupervisor>>,
    startup_leases: Mutex<HashMap<String, StartupTeamLease>>,
}

impl WorkerAckRegistry {
    pub(crate) fn arm(&self, tab_id: &str, attempt_id: &str) -> Receiver<WorkerAckSignal> {
        let (sender, receiver) = mpsc::channel();
        let previous = self.waiters.lock().insert(
            tab_id.to_string(),
            WorkerAckWaiter {
                attempt_id: attempt_id.to_string(),
                sender,
            },
        );
        if let Some(previous) = previous {
            let _ = previous.sender.send(WorkerAckSignal::Superseded);
        }
        receiver
    }

    pub(crate) fn acknowledge(&self, tab_id: &str, attempt_id: &str) -> bool {
        let waiter = {
            let mut waiters = self.waiters.lock();
            match waiters.get(tab_id) {
                Some(waiter) if waiter.attempt_id == attempt_id => waiters.remove(tab_id),
                _ => None,
            }
        };
        waiter.is_some_and(|waiter| waiter.sender.send(WorkerAckSignal::Acknowledged).is_ok())
    }

    pub(crate) fn expire(&self, tab_id: &str, attempt_id: &str) -> bool {
        let mut waiters = self.waiters.lock();
        if waiters
            .get(tab_id)
            .is_some_and(|waiter| waiter.attempt_id == attempt_id)
        {
            waiters.remove(tab_id);
            true
        } else {
            false
        }
    }

    pub(crate) fn arm_startup(&self, authority: StartupAuthority) -> Receiver<AgentStartupSignal> {
        let (sender, receiver) = mpsc::channel();
        let previous = self.startup_waiters.lock().insert(
            authority.child_tab_id.clone(),
            AgentStartupWaiter { authority, sender },
        );
        if let Some(previous) = previous {
            let _ = previous.sender.send(AgentStartupSignal::Superseded);
        }
        receiver
    }

    /// Retire the exact matching waiter and, when it carried a Team lease,
    /// release that lease: a started attempt makes its Team a normal Team.
    fn retire_startup_waiter(
        &self,
        tab_id: &str,
        attempt_id: &str,
        signal: AgentStartupSignal,
    ) -> Option<StartupAuthority> {
        let waiter = {
            let mut waiters = self.startup_waiters.lock();
            match waiters.get(tab_id) {
                Some(waiter) if waiter.authority.attempt_id == attempt_id => waiters.remove(tab_id),
                _ => None,
            }
        }?;
        let AgentStartupWaiter { authority, sender } = waiter;
        if let Some(team_id) = authority.team_id.as_deref() {
            self.release_startup_lease(team_id);
        }
        sender.send(signal).is_ok().then_some(authority)
    }

    pub(crate) fn mark_startup_ready(&self, tab_id: &str, attempt_id: &str) -> bool {
        self.retire_startup_waiter(tab_id, attempt_id, AgentStartupSignal::Ready)
            .is_some()
    }

    /// Authorize and hold under one lock so a replacement attempt armed
    /// concurrently can never be held by the previous attempt's creator.
    pub(crate) fn hold_startup_authorized(
        &self,
        caller_tab_id: &str,
        tab_id: &str,
        attempt_id: &str,
        child: Option<&ChildStartupState>,
    ) -> Result<StartupAuthority, (StartupHoldRefusal, Option<StartupAuthority>)> {
        let mut waiters = self.startup_waiters.lock();
        let supervised = waiters.get(tab_id).map(|waiter| waiter.authority.clone());
        let authority = match authorize_startup_hold(
            caller_tab_id,
            tab_id,
            attempt_id,
            supervised.as_ref(),
            child,
        ) {
            Ok(authority) => authority,
            Err(refusal) => return Err((refusal, supervised)),
        };
        let waiter = waiters.remove(tab_id).expect("waiter authorized above");
        drop(waiters);
        if let Some(team_id) = authority.team_id.as_deref() {
            self.release_startup_lease(team_id);
        }
        let _ = waiter.sender.send(AgentStartupSignal::Held);
        Ok(authority)
    }

    pub(crate) fn mark_startup_launch_command_finished(
        &self,
        tab_id: &str,
        attempt_id: &str,
        exit_code: Option<i32>,
    ) -> bool {
        self.startup_waiters
            .lock()
            .get(tab_id)
            .filter(|waiter| waiter.authority.attempt_id == attempt_id)
            .is_some_and(|waiter| {
                waiter
                    .sender
                    .send(AgentStartupSignal::LaunchCommandFinished(exit_code))
                    .is_ok()
            })
    }

    /// Retire the exact expired attempt, handing back its recorded ownership so
    /// only that attempt's terminal path can act. A stale timer gets `None`.
    pub(crate) fn expire_startup(
        &self,
        tab_id: &str,
        attempt_id: &str,
    ) -> Option<StartupAuthority> {
        let mut waiters = self.startup_waiters.lock();
        if waiters
            .get(tab_id)
            .is_some_and(|waiter| waiter.authority.attempt_id == attempt_id)
        {
            waiters.remove(tab_id).map(|waiter| waiter.authority)
        } else {
            None
        }
    }

    fn register_startup_supervisor(
        &self,
        tab_id: &str,
        supervisor_tab_id: &str,
        agent_title: &str,
        team_id: Option<&str>,
    ) {
        self.startup_supervisors.lock().insert(
            tab_id.to_string(),
            PendingAgentStartupSupervisor {
                supervisor_tab_id: supervisor_tab_id.to_string(),
                agent_title: agent_title.to_string(),
                team_id: team_id.map(str::to_string),
            },
        );
    }

    fn register_startup_lease(&self, lease: StartupTeamLease) {
        self.startup_leases
            .lock()
            .insert(lease.team_id.clone(), lease);
    }

    fn release_startup_lease(&self, team_id: &str) -> Option<StartupTeamLease> {
        self.startup_leases.lock().remove(team_id)
    }

    #[cfg(test)]
    fn active_startup_leases(&self) -> Vec<StartupTeamLease> {
        self.startup_leases.lock().values().cloned().collect()
    }

    /// Decides and consumes under the lease lock, never from a copy: a lease a
    /// concurrent Ready/Held retired is already gone. Mux guard is the caller's.
    fn reap_startup_leases(&self, mux: &mut MultiplexerState) -> Vec<String> {
        let mut leases = self.startup_leases.lock();
        let reaped = crate::service::startup_lease::reap_orphaned_leases(mux, leases.values());
        for team_id in &reaped {
            leases.remove(team_id);
        }
        reaped
    }

    fn has_startup_supervisor(&self, tab_id: &str) -> bool {
        self.startup_supervisors.lock().contains_key(tab_id)
    }

    fn take_startup_supervisor(&self, tab_id: &str) -> Option<PendingAgentStartupSupervisor> {
        self.startup_supervisors.lock().remove(tab_id)
    }

    fn cancel_startup_supervisor(&self, tab_id: &str) {
        self.startup_supervisors.lock().remove(tab_id);
    }
}

pub(crate) fn worker_ack_timeout_event(
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    attempt_id: &str,
    spawn_command: &str,
) -> Value {
    json!({
        "kind": "worker_ack_startup_failed",
        "plan_id": plan_id,
        "task_id": task_id,
        "tab_id": tab_id,
        "attempt_id": attempt_id,
        "deadline_seconds": (WORKER_ACK_TIMEOUT + WORKER_ACK_FINAL_TIMEOUT).as_secs(),
        "expected_command": "codeterm agent ack",
        "spawn_command": spawn_command,
        "tab_state": TAB_STATE_ALIVE_AWAITING_INSPECTION,
        "attempt_state": ATTEMPT_STATE_FINALIZED,
        "deadline_basis": {
            "measurement_method": COLD_START_MEASUREMENT_METHOD,
            "claude_cold_start_seconds": MEASURED_CLAUDE_COLD_START_SECS,
            "codex_cold_start_seconds": MEASURED_CODEX_COLD_START_SECS,
            "selected_phase_seconds": COLD_START_DEADLINE_SECS,
        },
        "impact": "Worker startup did not acknowledge; the attempt was finalized while its tab remains alive for inspection.",
        "fallback": "Inspect the surviving tab before dispatching a replacement Worker.",
    })
}

fn agent_startup_notice_event(tab_id: &str, title: &str, attempt_id: &str) -> Value {
    json!({
        "kind": "agent_startup_overdue",
        "tab_id": tab_id,
        "title": title,
        "attempt_id": attempt_id,
        "deadline_seconds": AGENT_STARTUP_NOTICE_TIMEOUT.as_secs(),
        "tab_state": TAB_STATE_ALIVE_AWAITING_INSPECTION,
        "deadline_basis": {
            "measurement_method": COLD_START_MEASUREMENT_METHOD,
            "claude_cold_start_seconds": MEASURED_CLAUDE_COLD_START_SECS,
            "codex_cold_start_seconds": MEASURED_CODEX_COLD_START_SECS,
            "selected_phase_seconds": COLD_START_DEADLINE_SECS,
        },
        "impact": "The spawned agent has not acknowledged its startup contract. The tab may be waiting on a provider prompt.",
        "inspect": [
            format!("codeterm tab status --tab {tab_id}"),
            format!("codeterm tab get-text --tab {tab_id}"),
        ],
        "respond": format!("codeterm tab respond OPTION --tab {tab_id}"),
        "hold": format!("codeterm agent startup-hold --tab {tab_id} --attempt {attempt_id}"),
        "fallback": "Inspect first. If the exact provider picker remains visible after tab respond, use one bounded send --actions sequence derived from its current selection. Never resend the assigned task payload.",
    })
}

fn agent_startup_failed_event(
    tab_id: &str,
    title: &str,
    attempt_id: &str,
    exit_code: Option<i32>,
    provider_output: Option<&str>,
) -> Value {
    json!({
        "kind": "agent_startup_failed",
        "tab_id": tab_id,
        "title": title,
        "attempt_id": attempt_id,
        "deadline_seconds": (AGENT_STARTUP_NOTICE_TIMEOUT + AGENT_STARTUP_FINAL_TIMEOUT).as_secs(),
        "tab_state": TAB_STATE_ALIVE_AWAITING_INSPECTION,
        "deadline_basis": {
            "measurement_method": COLD_START_MEASUREMENT_METHOD,
            "claude_cold_start_seconds": MEASURED_CLAUDE_COLD_START_SECS,
            "codex_cold_start_seconds": MEASURED_CODEX_COLD_START_SECS,
            "selected_phase_seconds": COLD_START_DEADLINE_SECS,
        },
        "provider": { "exit_code": exit_code, "output": provider_output },
        "impact": "Provider startup expired; the tab remains alive for inspection.",
        "fallback": "Inspect the surviving tab before spawning a replacement.",
    })
}

impl ServiceContext {
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
        let message = payload.to_string();
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
    ) -> Result<bool, String> {
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
        Ok(self
            .worker_ack
            .mark_startup_ready(caller_tab_id, attempt_id))
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
            Ok(authority) => Ok(json!({
                "ok": true,
                "held": true,
                "state": "held_by_creator",
                "tab_id": authority.child_tab_id,
                "attempt_id": authority.attempt_id,
                "authority": authority.provenance(),
            })),
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
            if let Err(error) = service.deliver_worker_ack_reminder(&agent_tab_id, &attempt_id) {
                log::warn!(
                    "[agent-startup] binding reminder delivery failed attempt={attempt_id}: {error}"
                );
            }
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
            let provider_output = service
                .screen_state
                .lock()
                .get_screen_text(&attempt_id)
                .map(|output| {
                    crate::service::launch_diagnostics::redact_secrets(
                        &crate::service::launch_diagnostics::tail_bytes(&output, 4096),
                    )
                })
                .filter(|output| !output.trim().is_empty());
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
            let session_id = tab.agent_session_id.clone()?.trim().to_string();
            let nonce = tab.session_binding_nonce.clone()?;
            (!session_id.is_empty()
                && tab.reported_session_binding_nonce.as_deref() == Some(nonce.as_str()))
            .then_some((session_id, nonce))
        })
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
        let message = payload.to_string();
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
        let receiver = self.worker_ack.arm(worker_tab_id, attempt_id);
        let service = Arc::clone(self);
        let parent_tab_id = parent_tab_id.to_string();
        let worker_tab_id = worker_tab_id.to_string();
        let worker_title = worker_title.to_string();
        let plan_id = plan_id.to_string();
        let task_id = task_id.to_string();
        let attempt_id = attempt_id.to_string();
        std::thread::spawn(move || {
            match receiver.recv_timeout(WORKER_ACK_TIMEOUT) {
                Ok(WorkerAckSignal::Acknowledged) | Ok(WorkerAckSignal::Superseded) => return,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
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
            if let Err(error) = service.deliver_worker_ack_reminder(&worker_tab_id, &attempt_id) {
                log::warn!("[worker-ack] reminder delivery failed attempt={attempt_id}: {error}");
            }
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
            match receiver.recv_timeout(WORKER_ACK_FINAL_TIMEOUT) {
                Ok(WorkerAckSignal::Acknowledged) | Ok(WorkerAckSignal::Superseded) => return,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
            if !service.worker_ack.expire(&worker_tab_id, &attempt_id) {
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
            if let Err(error) = service.deliver_worker_ack_reminder(&worker_tab_id, &attempt_id) {
                log::warn!(
                    "[worker-ack] final binding reminder delivery failed attempt={attempt_id}: {error}"
                );
            }
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

fn worker_ack_reminder_message(tab_id: &str, attempt_id: &str) -> OutboundMessage {
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

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests;
