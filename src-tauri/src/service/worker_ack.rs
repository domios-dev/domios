use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Value};

use crate::multiplexer::state::MultiplexerState;
use crate::service::startup_lease::{
    authorize_startup_hold, ChildStartupState, StartupAuthority, StartupHoldRefusal,
    StartupTeamLease,
};

use super::ServiceContext;

#[path = "worker_ack/registry.rs"]
mod registry;
#[path = "worker_ack/plan_deadline.rs"]
mod plan_deadline;
pub(crate) use registry::{WorkerAckArm, WorkerAckIdentity};
use registry::WorkerAckState;
pub(crate) use plan_deadline::worker_ack_reminder_message;

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
    StartupHeld,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentStartupSignal {
    Ready,
    Held,
    LaunchCommandFinished(Option<i32>),
    Superseded,
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
enum WorkerAckWaitOutcome {
    Resolved,
    Overdue,
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

fn await_worker_ack(
    receiver: &Receiver<WorkerAckSignal>,
    timeout: Duration,
) -> WorkerAckWaitOutcome {
    let mut deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return WorkerAckWaitOutcome::Overdue;
        }
        match receiver.recv_timeout(remaining) {
            Ok(WorkerAckSignal::Acknowledged | WorkerAckSignal::Superseded)
            | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return WorkerAckWaitOutcome::Resolved;
            }
            Ok(WorkerAckSignal::StartupHeld) => {
                deadline = std::time::Instant::now() + timeout;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                return WorkerAckWaitOutcome::Overdue;
            }
        }
    }
}

struct WorkerAckWaiter {
    observers: Vec<Sender<WorkerAckSignal>>,
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
    waiters: Mutex<WorkerAckState>,
    startup_waiters: Mutex<HashMap<String, AgentStartupWaiter>>,
    startup_acknowledged: Mutex<HashMap<String, String>>,
    startup_supervisors: Mutex<HashMap<String, PendingAgentStartupSupervisor>>,
    startup_leases: Mutex<HashMap<String, StartupTeamLease>>,
}

impl WorkerAckRegistry {
    pub(crate) fn arm_startup(&self, authority: StartupAuthority) -> Receiver<AgentStartupSignal> {
        let (sender, receiver) = mpsc::channel();
        let mut startup_waiters = self.startup_waiters.lock();
        self.startup_acknowledged
            .lock()
            .remove(&authority.child_tab_id);
        let previous = startup_waiters.insert(
            authority.child_tab_id.clone(),
            AgentStartupWaiter { authority, sender },
        );
        drop(startup_waiters);
        if let Some(previous) = previous {
            let _ = previous.sender.send(AgentStartupSignal::Superseded);
        }
        receiver
    }

    pub(crate) fn mark_startup_ready(&self, tab_id: &str, attempt_id: &str) -> bool {
        self.acknowledge_startup(tab_id, attempt_id).is_some()
    }

    pub(crate) fn acknowledge_startup(
        &self,
        tab_id: &str,
        attempt_id: &str,
    ) -> Option<AgentStartupAcknowledgement> {
        let (acknowledgement, team_id) = {
            let mut waiters = self.startup_waiters.lock();
            if waiters
                .get(tab_id)
                .is_some_and(|waiter| waiter.authority.attempt_id == attempt_id)
            {
                let waiter = waiters.remove(tab_id)?;
                let AgentStartupWaiter { authority, sender } = waiter;
                let _ = sender.send(AgentStartupSignal::Ready);
                self.startup_acknowledged
                    .lock()
                    .insert(tab_id.to_string(), attempt_id.to_string());
                (AgentStartupAcknowledgement::Acknowledged, authority.team_id)
            } else {
                let acknowledged = self.startup_acknowledged.lock();
                if acknowledged.get(tab_id).is_some_and(|id| id == attempt_id) {
                    (AgentStartupAcknowledgement::AlreadyAcknowledged, None)
                } else {
                    return None;
                }
            }
        };
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

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests;
