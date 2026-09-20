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
    StartupHoldRejection, StartupTaskIdentity, StartupTeamLease,
};

use super::ServiceContext;

#[path = "worker_ack/deadlines.rs"]
mod deadlines;

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

fn interactive_child_pty_recovery(tab_id: &str, attempt_id: &str) -> Value {
    json!({
        "capability": crate::prompts::INTERACTIVE_CHILD_PTY_CAPABILITY_ID,
        "target_tab_id": tab_id,
        "attempt_id": attempt_id,
        "primitives": [
            format!("codeterm tab inspect --tab {tab_id}"),
            format!("codeterm tab status --tab {tab_id}"),
            format!("codeterm tab get-text --tab {tab_id}"),
            format!("codeterm tab respond OPTION --tab {tab_id}"),
            format!("codeterm send --tab {tab_id} --actions <bounded-visible-state-derived-actions>"),
        ],
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkerAckSignal {
    Acknowledged,
    Superseded,
    DeadlineExtended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentStartupSignal {
    Ready,
    Held,
    LaunchCommandFinished(Option<i32>),
    Superseded,
    DeadlineExtended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentStartupAcknowledgement {
    Acknowledged,
    AlreadyAcknowledged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentStartupNoticeOutcome {
    Resolved,
    Overdue { exit_code: Option<i32> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkerAckNoticeOutcome {
    Acknowledged,
    Superseded,
    Overdue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WaiterExpiry {
    Expired,
    Extended,
    NoLongerCurrent,
}

fn await_worker_ack_notice(
    receiver: &Receiver<WorkerAckSignal>,
    timeout: Duration,
    generation: &mut u64,
) -> WorkerAckNoticeOutcome {
    let mut deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return WorkerAckNoticeOutcome::Overdue;
        }
        match receiver.recv_timeout(remaining) {
            Ok(WorkerAckSignal::Acknowledged) => return WorkerAckNoticeOutcome::Acknowledged,
            Ok(WorkerAckSignal::Superseded)
            | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return WorkerAckNoticeOutcome::Superseded;
            }
            Ok(WorkerAckSignal::DeadlineExtended) => {
                *generation = generation.wrapping_add(1);
                deadline = std::time::Instant::now() + timeout;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                return WorkerAckNoticeOutcome::Overdue;
            }
        }
    }
}

fn await_agent_startup_notice(
    receiver: &Receiver<AgentStartupSignal>,
    timeout: Duration,
    generation: &mut u64,
) -> AgentStartupNoticeOutcome {
    let mut deadline = std::time::Instant::now() + timeout;
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
            Ok(AgentStartupSignal::DeadlineExtended) => {
                *generation = generation.wrapping_add(1);
                deadline = std::time::Instant::now() + timeout;
            }
            Ok(AgentStartupSignal::LaunchCommandFinished(code)) => exit_code = code,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                return AgentStartupNoticeOutcome::Overdue { exit_code };
            }
        }
    }
}

struct WorkerAckWaiter {
    identity: WorkerAckIdentity,
    deadline_generation: u64,
    sender: Sender<WorkerAckSignal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkerAckIdentity {
    plan_id: String,
    task_id: String,
    attempt_id: String,
    tab_id: String,
    session_id: Option<String>,
    binding_nonce: Option<String>,
}

pub(crate) fn startup_task_identity_from_env(
    env: &HashMap<String, String>,
) -> Option<StartupTaskIdentity> {
    Some(StartupTaskIdentity {
        plan_id: env.get("CODETERM_PLAN_ID")?.clone(),
        task_id: env.get("CODETERM_TASK_ID")?.clone(),
        attempt_id: env.get("CODETERM_ATTEMPT_ID")?.clone(),
    })
}

enum WorkerAckArm {
    Armed(Receiver<WorkerAckSignal>),
    Extended,
    AlreadyAcknowledged,
}

enum AgentStartupArm {
    Armed(Receiver<AgentStartupSignal>),
    Extended,
    AlreadyAcknowledged,
}

struct AgentStartupWaiter {
    authority: StartupAuthority,
    deadline_generation: u64,
    sender: Sender<AgentStartupSignal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingAgentStartupSupervisor {
    supervisor_tab_id: String,
    agent_title: String,
    team_id: Option<String>,
    task_identity: Option<StartupTaskIdentity>,
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
    worker_acknowledged: Mutex<HashMap<String, WorkerAckIdentity>>,
    startup_waiters: Mutex<HashMap<String, AgentStartupWaiter>>,
    startup_acknowledged: Mutex<HashMap<String, StartupAuthority>>,
    startup_supervisors: Mutex<HashMap<String, PendingAgentStartupSupervisor>>,
    startup_leases: Mutex<HashMap<String, StartupTeamLease>>,
}

impl WorkerAckRegistry {
    fn arm(&self, identity: WorkerAckIdentity) -> WorkerAckArm {
        let tab_id = identity.tab_id.clone();
        let mut waiters = self.waiters.lock();
        if let Some(waiter) = waiters
            .get_mut(&tab_id)
            .filter(|waiter| waiter.identity == identity)
        {
            if waiter
                .sender
                .send(WorkerAckSignal::DeadlineExtended)
                .is_ok()
            {
                waiter.deadline_generation = waiter.deadline_generation.wrapping_add(1);
                return WorkerAckArm::Extended;
            }
        }
        let mut acknowledged = self.worker_acknowledged.lock();
        if acknowledged.get(&tab_id) == Some(&identity) {
            return WorkerAckArm::AlreadyAcknowledged;
        }
        acknowledged.remove(&tab_id);
        let (sender, receiver) = mpsc::channel();
        let previous = waiters.insert(
            tab_id,
            WorkerAckWaiter {
                identity,
                deadline_generation: 0,
                sender,
            },
        );
        drop(acknowledged);
        drop(waiters);
        if let Some(previous) = previous {
            let _ = previous.sender.send(WorkerAckSignal::Superseded);
        }
        WorkerAckArm::Armed(receiver)
    }

    pub(crate) fn acknowledge(&self, tab_id: &str, attempt_id: &str) -> bool {
        let waiter = {
            let mut waiters = self.waiters.lock();
            match waiters.get(tab_id) {
                Some(waiter) if waiter.identity.attempt_id == attempt_id => waiters.remove(tab_id),
                _ => None,
            }
        };
        if let Some(waiter) = waiter {
            self.worker_acknowledged
                .lock()
                .insert(tab_id.to_string(), waiter.identity);
            let _ = waiter.sender.send(WorkerAckSignal::Acknowledged);
            return true;
        }
        self.worker_acknowledged
            .lock()
            .get(tab_id)
            .is_some_and(|identity| identity.attempt_id == attempt_id)
    }

    fn expire(&self, identity: &WorkerAckIdentity, generation: u64) -> WaiterExpiry {
        let mut waiters = self.waiters.lock();
        let Some(waiter) = waiters
            .get(&identity.tab_id)
            .filter(|waiter| waiter.identity == *identity)
        else {
            return WaiterExpiry::NoLongerCurrent;
        };
        if waiter.deadline_generation != generation {
            return WaiterExpiry::Extended;
        }
        waiters.remove(&identity.tab_id);
        WaiterExpiry::Expired
    }

    fn arm_startup(&self, authority: StartupAuthority) -> AgentStartupArm {
        let (sender, receiver) = mpsc::channel();
        let mut startup_waiters = self.startup_waiters.lock();
        if let Some(waiter) = startup_waiters
            .get_mut(&authority.child_tab_id)
            .filter(|waiter| waiter.authority == authority)
        {
            if waiter
                .sender
                .send(AgentStartupSignal::DeadlineExtended)
                .is_ok()
            {
                waiter.deadline_generation = waiter.deadline_generation.wrapping_add(1);
                return AgentStartupArm::Extended;
            }
        }
        let mut acknowledged = self.startup_acknowledged.lock();
        if acknowledged.get(&authority.child_tab_id) == Some(&authority) {
            return AgentStartupArm::AlreadyAcknowledged;
        }
        acknowledged.remove(&authority.child_tab_id);
        let previous = startup_waiters.insert(
            authority.child_tab_id.clone(),
            AgentStartupWaiter {
                authority,
                deadline_generation: 0,
                sender,
            },
        );
        drop(acknowledged);
        drop(startup_waiters);
        if let Some(previous) = previous {
            let _ = previous.sender.send(AgentStartupSignal::Superseded);
        }
        AgentStartupArm::Armed(receiver)
    }

    pub(crate) fn mark_startup_ready(&self, tab_id: &str, attempt_id: &str) -> bool {
        let identity = self
            .startup_waiters
            .lock()
            .get(tab_id)
            .filter(|waiter| waiter.authority.attempt_id == attempt_id)
            .map(|waiter| waiter.authority.clone());
        identity.is_some_and(|authority| {
            self.acknowledge_startup(
                tab_id,
                attempt_id,
                authority.session_id.as_deref(),
                authority.binding_nonce.as_deref(),
            )
            .is_some()
        })
    }

    pub(crate) fn acknowledge_startup(
        &self,
        tab_id: &str,
        attempt_id: &str,
        session_id: Option<&str>,
        binding_nonce: Option<&str>,
    ) -> Option<AgentStartupAcknowledgement> {
        let (acknowledgement, team_id, completed_authority) = {
            let mut waiters = self.startup_waiters.lock();
            if waiters.get(tab_id).is_some_and(|waiter| {
                waiter.authority.attempt_id == attempt_id
                    && waiter.authority.session_id.as_deref() == session_id
                    && waiter.authority.binding_nonce.as_deref() == binding_nonce
            }) {
                let waiter = waiters.remove(tab_id)?;
                let AgentStartupWaiter {
                    authority, sender, ..
                } = waiter;
                let _ = sender.send(AgentStartupSignal::Ready);
                self.startup_acknowledged
                    .lock()
                    .insert(tab_id.to_string(), authority.clone());
                (
                    AgentStartupAcknowledgement::Acknowledged,
                    authority.team_id.clone(),
                    Some(authority),
                )
            } else {
                let acknowledged = self.startup_acknowledged.lock();
                if acknowledged.get(tab_id).is_some_and(|authority| {
                    authority.attempt_id == attempt_id
                        && authority.session_id.as_deref() == session_id
                        && authority.binding_nonce.as_deref() == binding_nonce
                }) {
                    (AgentStartupAcknowledgement::AlreadyAcknowledged, None, None)
                } else {
                    return None;
                }
            }
        };
        if let Some(authority) = completed_authority.as_ref() {
            self.clear_startup_supervisor(authority);
        }
        if let Some(team_id) = team_id.as_deref() {
            self.release_startup_lease(team_id);
        }
        Some(acknowledgement)
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
        self.clear_startup_supervisor(&authority);
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

    /// Retire the exact expired attempt only if no later re-arm extended it.
    pub(crate) fn expire_startup(
        &self,
        authority: &StartupAuthority,
        generation: u64,
    ) -> WaiterExpiry {
        let mut waiters = self.startup_waiters.lock();
        let Some(waiter) = waiters
            .get(&authority.child_tab_id)
            .filter(|waiter| waiter.authority == *authority)
        else {
            return WaiterExpiry::NoLongerCurrent;
        };
        if waiter.deadline_generation != generation {
            return WaiterExpiry::Extended;
        }
        let expired = waiters
            .remove(&authority.child_tab_id)
            .map(|waiter| waiter.authority);
        drop(waiters);
        if let Some(expired) = expired {
            self.clear_startup_supervisor(&expired);
            WaiterExpiry::Expired
        } else {
            WaiterExpiry::NoLongerCurrent
        }
    }

    fn register_startup_supervisor(
        &self,
        tab_id: &str,
        supervisor_tab_id: &str,
        agent_title: &str,
        team_id: Option<&str>,
        task_identity: Option<StartupTaskIdentity>,
    ) {
        let next = PendingAgentStartupSupervisor {
            supervisor_tab_id: supervisor_tab_id.to_string(),
            agent_title: agent_title.to_string(),
            team_id: team_id.map(str::to_string),
            task_identity,
        };
        let mut supervisors = self.startup_supervisors.lock();
        if supervisors.get(tab_id) != Some(&next) {
            supervisors.insert(tab_id.to_string(), next);
        }
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

    fn startup_supervisor(&self, tab_id: &str) -> Option<PendingAgentStartupSupervisor> {
        self.startup_supervisors.lock().get(tab_id).cloned()
    }

    fn clear_startup_supervisor(&self, authority: &StartupAuthority) {
        let mut supervisors = self.startup_supervisors.lock();
        if supervisors
            .get(&authority.child_tab_id)
            .is_some_and(|supervisor| {
                supervisor.supervisor_tab_id == authority.creator_tab_id
                    && supervisor.team_id == authority.team_id
                    && supervisor.task_identity == authority.task_identity
            })
        {
            supervisors.remove(&authority.child_tab_id);
        }
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
        "recovery": interactive_child_pty_recovery(tab_id, attempt_id),
        "deadline_basis": {
            "measurement_method": COLD_START_MEASUREMENT_METHOD,
            "claude_cold_start_seconds": MEASURED_CLAUDE_COLD_START_SECS,
            "codex_cold_start_seconds": MEASURED_CODEX_COLD_START_SECS,
            "selected_phase_seconds": COLD_START_DEADLINE_SECS,
        },
        "impact": "Worker startup did not acknowledge; the attempt was finalized while its tab remains alive for inspection.",
        "fallback": "Invoke recovery.capability for this exact tab and attempt before dispatching a replacement Worker.",
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
        "recovery": interactive_child_pty_recovery(tab_id, attempt_id),
        "fallback": "Invoke recovery.capability for this exact tab and attempt. Never resend the assigned task payload.",
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
        "recovery": interactive_child_pty_recovery(tab_id, attempt_id),
        "deadline_basis": {
            "measurement_method": COLD_START_MEASUREMENT_METHOD,
            "claude_cold_start_seconds": MEASURED_CLAUDE_COLD_START_SECS,
            "codex_cold_start_seconds": MEASURED_CODEX_COLD_START_SECS,
            "selected_phase_seconds": COLD_START_DEADLINE_SECS,
        },
        "provider": { "exit_code": exit_code, "output": provider_output },
        "impact": "Provider startup expired; the tab remains alive for inspection.",
        "fallback": "Invoke recovery.capability for this exact tab and attempt before spawning a replacement.",
    })
}

fn compact_notice_text(value: &str, max_chars: usize) -> String {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    value.chars().take(max_chars).collect()
}

fn compact_provider_failure(value: &str) -> String {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars = value.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(120);
    chars[start..].iter().collect::<String>()
}

fn worker_lifecycle_notice(
    title: &str,
    provider: Option<&str>,
    provider_output: Option<&str>,
    payload: &Value,
) -> String {
    let title = compact_notice_text(title, 32);
    let provider = provider
        .map(|provider| compact_notice_text(provider, 32))
        .filter(|provider| !provider.is_empty())
        .unwrap_or_else(|| "unknown provider".to_string());
    let attempt_id = payload
        .get("attempt_id")
        .and_then(Value::as_str)
        .map(|attempt| compact_notice_text(attempt, 40))
        .filter(|attempt| !attempt.is_empty())
        .unwrap_or_else(|| "unknown attempt".to_string());
    let task_id = payload
        .get("task_id")
        .and_then(Value::as_str)
        .map(|task| compact_notice_text(task, 40));
    let kind = payload
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let action = match kind {
        "worker_ack" => format!(
            "acknowledged task {}",
            task_id.as_deref().unwrap_or("unknown task")
        ),
        "worker_ack_startup_failed" => format!(
            "did not acknowledge task {}",
            task_id.as_deref().unwrap_or("unknown task")
        ),
        "agent_startup_failed" => "failed to start".to_string(),
        "agent_startup_overdue" => "is still starting".to_string(),
        _ => "reported a lifecycle update".to_string(),
    };
    let detail = if matches!(kind, "agent_startup_failed" | "worker_ack_startup_failed") {
        payload
            .pointer("/provider/output")
            .and_then(Value::as_str)
            .map(compact_provider_failure)
            .filter(|detail| !detail.is_empty())
            .or_else(|| provider_output.map(compact_provider_failure))
            .filter(|detail| !detail.is_empty())
            .or_else(|| {
                payload
                    .pointer("/provider/exit_code")
                    .and_then(Value::as_i64)
                    .map(|code| format!("provider exit code {code}"))
            })
    } else {
        None
    };
    let message = match detail {
        Some(detail) => {
            format!("Agent {title} ({provider}) {action}, attempt {attempt_id}: {detail}")
        }
        None => format!("Agent {title} ({provider}) {action}, attempt {attempt_id}."),
    };
    compact_notice_text(&message, 280)
}

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
        task_identity: Option<StartupTaskIdentity>,
    ) {
        self.worker_ack.register_startup_supervisor(
            agent_tab_id,
            supervisor_tab_id,
            agent_title,
            None,
            task_identity,
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
                None,
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

    pub(crate) fn prepare_agent_startup_supervision(
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
            )
        };
        self.register_agent_startup_supervision(
            &supervisor.0,
            agent_tab_id,
            &supervisor.1,
            task_identity,
        );
        true
    }

    pub(crate) fn activate_agent_startup_supervision(
        &self,
        agent_tab_id: &str,
        attempt_id: &str,
        binding_nonce: &str,
        expected_task_identity: Option<&StartupTaskIdentity>,
    ) -> Result<(), String> {
        let (task_bound, session_id, task_binding) = {
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
            (
                expected_task_identity.is_some()
                    || task_binding.as_ref().is_some_and(|binding| {
                        binding.plan_id.is_some() && binding.task_id.is_some()
                    }),
                tab.agent_session_id.clone(),
                task_binding,
            )
        };
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
        self.arm_agent_startup_deadline(
            StartupAuthority::new(&supervisor.supervisor_tab_id, agent_tab_id, attempt_id)
                .in_team(supervisor.team_id.clone())
                .for_task(supervisor.task_identity.clone())
                .for_binding(session_id.as_deref(), binding_nonce),
            &supervisor.agent_title,
        );
        Ok(())
    }

    fn child_startup_state(&self, tab_id: &str) -> Option<ChildStartupState> {
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
        let session_id = self.mux.read().get_tab(caller_tab_id).and_then(|tab| {
            startup_ack_owns_attempt(tab, attempt_id, binding_nonce)
                .then(|| tab.agent_session_id.clone())
        });
        let Some(session_id) = session_id else {
            return Err(format!(
                "startup acknowledgement does not own attempt {attempt_id} for Tab {caller_tab_id}"
            ));
        };
        self.worker_ack
            .acknowledge_startup(
                caller_tab_id,
                attempt_id,
                session_id.as_deref(),
                Some(binding_nonce),
            )
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
