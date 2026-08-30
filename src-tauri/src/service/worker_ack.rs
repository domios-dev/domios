use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Value};

use crate::agent::reports::ReportDetails;

use super::ServiceContext;

const WORKER_ACK_TIMEOUT: Duration = Duration::from_secs(60);
const WORKER_ACK_FINAL_TIMEOUT: Duration = Duration::from_secs(60);
const AGENT_STARTUP_NOTICE_TIMEOUT: Duration = Duration::from_secs(30);
const AGENT_STARTUP_FINAL_TIMEOUT: Duration = Duration::from_secs(30);
const WORKER_ACK_REMINDER: &str = "Acknowledge this Worker assignment with codeterm agent ack before doing any work. If acknowledgement is still absent at the final bounded startup deadline, this tab will be cleaned up.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkerAckFirstDeadline {
    SystemReminder,
    StartupUnbound,
}

fn first_deadline_action(authenticated_attempt_binding: bool) -> WorkerAckFirstDeadline {
    if authenticated_attempt_binding {
        WorkerAckFirstDeadline::SystemReminder
    } else {
        WorkerAckFirstDeadline::StartupUnbound
    }
}

fn close_worker_after_final_deadline(
    close_tab: impl FnOnce() -> Result<(), String>,
) -> Option<String> {
    close_tab().err()
}

fn serialize_agent_startup_cleanup<T>(
    registry: &WorkerAckRegistry,
    close_tab: impl FnOnce() -> T,
) -> T {
    let _cleanup = registry.startup_cleanup.lock();
    close_tab()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkerAckSignal {
    Acknowledged,
    Superseded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentStartupSignal {
    Ready,
    Held,
    ProcessExited,
    Superseded,
}

struct WorkerAckWaiter {
    attempt_id: String,
    sender: Sender<WorkerAckSignal>,
}

struct AgentStartupWaiter {
    attempt_id: String,
    sender: Sender<AgentStartupSignal>,
}

struct PendingAgentStartupSupervisor {
    supervisor_tab_id: String,
    agent_title: String,
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
    startup_cleanup: Mutex<()>,
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

    pub(crate) fn arm_startup(&self, tab_id: &str, attempt_id: &str) -> Receiver<AgentStartupSignal> {
        let (sender, receiver) = mpsc::channel();
        let previous = self.startup_waiters.lock().insert(
            tab_id.to_string(),
            AgentStartupWaiter {
                attempt_id: attempt_id.to_string(),
                sender: sender.clone(),
            },
        );
        if let Some(previous) = previous {
            let _ = previous.sender.send(AgentStartupSignal::Superseded);
        }
        receiver
    }

    pub(crate) fn mark_startup_ready(&self, tab_id: &str, attempt_id: &str) -> bool {
        let waiter = {
            let mut waiters = self.startup_waiters.lock();
            match waiters.get(tab_id) {
                Some(waiter) if waiter.attempt_id == attempt_id => waiters.remove(tab_id),
                _ => None,
            }
        };
        waiter.is_some_and(|waiter| waiter.sender.send(AgentStartupSignal::Ready).is_ok())
    }

    pub(crate) fn hold_startup(&self, tab_id: &str, attempt_id: &str) -> bool {
        let waiter = {
            let mut waiters = self.startup_waiters.lock();
            match waiters.get(tab_id) {
                Some(waiter) if waiter.attempt_id == attempt_id => waiters.remove(tab_id),
                _ => None,
            }
        };
        waiter.is_some_and(|waiter| waiter.sender.send(AgentStartupSignal::Held).is_ok())
    }

    pub(crate) fn mark_startup_process_exited(&self, tab_id: &str, attempt_id: &str) -> bool {
        self.startup_waiters
            .lock()
            .get(tab_id)
            .filter(|waiter| waiter.attempt_id == attempt_id)
            .is_some_and(|waiter| waiter.sender.send(AgentStartupSignal::ProcessExited).is_ok())
    }

    pub(crate) fn expire_startup(&self, tab_id: &str, attempt_id: &str) -> bool {
        let mut waiters = self.startup_waiters.lock();
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

    fn register_startup_supervisor(
        &self,
        tab_id: &str,
        supervisor_tab_id: &str,
        agent_title: &str,
    ) {
        self.startup_supervisors.lock().insert(
            tab_id.to_string(),
            PendingAgentStartupSupervisor {
                supervisor_tab_id: supervisor_tab_id.to_string(),
                agent_title: agent_title.to_string(),
            },
        );
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
    cleanup_error: Option<&str>,
) -> Value {
    let cleanup_ok = cleanup_error.is_none();
    json!({
        "kind": if cleanup_ok { "worker_ack_startup_failed" } else { "worker_ack_cleanup_failed" },
        "plan_id": plan_id,
        "task_id": task_id,
        "tab_id": tab_id,
        "attempt_id": attempt_id,
        "deadline_seconds": (WORKER_ACK_TIMEOUT + WORKER_ACK_FINAL_TIMEOUT).as_secs(),
        "expected_command": "codeterm agent ack",
        "spawn_command": spawn_command,
        "cleanup": { "ok": cleanup_ok, "error": cleanup_error },
        "impact": if cleanup_ok {
            "Worker startup failed; the exact attempt was closed and its tab assignment was cleared."
        } else {
            "ORCHESTRATION PRODUCT INCIDENT: the startup attempt was fenced, but its tab could not be closed."
        },
        "fallback": if cleanup_ok {
            "Inspect the preserved lane before dispatching a replacement Worker."
        } else {
            "Treat the tab as still live and close it explicitly before dispatching a replacement Worker."
        },
    })
}

fn agent_startup_notice_event(tab_id: &str, title: &str, attempt_id: &str) -> Value {
    json!({
        "kind": "agent_startup_overdue",
        "tab_id": tab_id,
        "title": title,
        "attempt_id": attempt_id,
        "deadline_seconds": AGENT_STARTUP_NOTICE_TIMEOUT.as_secs(),
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
    cleanup_error: Option<&str>,
) -> Value {
    let cleanup_ok = cleanup_error.is_none();
    json!({
        "kind": if cleanup_ok { "agent_startup_failed" } else { "agent_startup_cleanup_failed" },
        "tab_id": tab_id,
        "title": title,
        "attempt_id": attempt_id,
        "deadline_seconds": (AGENT_STARTUP_NOTICE_TIMEOUT + AGENT_STARTUP_FINAL_TIMEOUT).as_secs(),
        "cleanup": { "ok": cleanup_ok, "error": cleanup_error },
        "impact": if cleanup_ok {
            "The exact spawned attempt did not acknowledge startup; its tab was closed at the bounded cleanup deadline."
        } else {
            "ORCHESTRATION PRODUCT INCIDENT: provider startup expired, but the tab could not be closed."
        },
        "fallback": if cleanup_ok {
            "Inspect the provider and spawn a replacement only after fixing the startup prompt or provider failure."
        } else {
            "Treat the tab as live and close it explicitly before spawning a replacement."
        },
    })
}

fn agent_startup_process_exited_event(tab_id: &str, title: &str, attempt_id: &str) -> Value {
    json!({
        "kind": "agent_startup_process_exited",
        "tab_id": tab_id,
        "title": title,
        "attempt_id": attempt_id,
        "impact": "The exact provider launch command ended before its startup acknowledgement.",
        "hold": format!("codeterm agent startup-hold --tab {tab_id} --attempt {attempt_id}"),
        "fallback": "Inspect the tab. Hold this exact attempt if its shell must be preserved; otherwise CodeTerm will close it at the bounded cleanup deadline.",
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
        );
    }

    pub(crate) fn cancel_agent_startup_supervision(&self, agent_tab_id: &str) {
        self.worker_ack.cancel_startup_supervisor(agent_tab_id);
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

    pub(crate) fn activate_agent_startup_supervision(
        &self,
        agent_tab_id: &str,
        attempt_id: &str,
    ) {
        let Some(supervisor) = self.worker_ack.take_startup_supervisor(agent_tab_id) else {
            return;
        };
        self.arm_agent_startup_deadline(
            &supervisor.supervisor_tab_id,
            agent_tab_id,
            &supervisor.agent_title,
            attempt_id,
        );
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
        let envelope = crate::service::messenger::MessageEnvelope::report(agent_title, agent_tab_id);
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

    pub(crate) fn observe_agent_startup_process_exited(&self, tab_id: &str, attempt_id: &str) {
        self.worker_ack
            .mark_startup_process_exited(tab_id, attempt_id);
    }

    pub(crate) fn hold_agent_startup(
        &self,
        caller_tab_id: &str,
        tab_id: &str,
        attempt_id: &str,
    ) -> Result<bool, String> {
        let caller_is_general_agent = self
            .mux
            .read()
            .get_tab(caller_tab_id)
            .is_some_and(crate::service::general_agent::is_general_agent);
        let authorized = self.mux.read().get_tab(tab_id).is_some_and(|tab| {
            tab.pty_id.as_deref() == Some(attempt_id)
                && (tab.parent_tab_id.as_deref() == Some(caller_tab_id)
                    || tab.spawned_by_tab_id.as_deref() == Some(caller_tab_id)
                    || caller_is_general_agent)
        });
        if !authorized {
            return Err(format!(
                "Tab {caller_tab_id} cannot hold startup attempt {attempt_id} for Tab {tab_id}"
            ));
        }
        Ok(self.worker_ack.hold_startup(tab_id, attempt_id))
    }

    pub(crate) fn arm_agent_startup_deadline(
        &self,
        supervisor_tab_id: &str,
        agent_tab_id: &str,
        agent_title: &str,
        attempt_id: &str,
    ) {
        let receiver = self.worker_ack.arm_startup(agent_tab_id, attempt_id);
        let service = self.clone();
        let supervisor_tab_id = supervisor_tab_id.to_string();
        let agent_tab_id = agent_tab_id.to_string();
        let agent_title = agent_title.to_string();
        let attempt_id = attempt_id.to_string();
        std::thread::spawn(move || {
            let final_deadline = std::time::Instant::now()
                + AGENT_STARTUP_NOTICE_TIMEOUT
                + AGENT_STARTUP_FINAL_TIMEOUT;
            let mut process_exited = false;
            match receiver.recv_timeout(AGENT_STARTUP_NOTICE_TIMEOUT) {
                Ok(AgentStartupSignal::Ready | AgentStartupSignal::Held | AgentStartupSignal::Superseded) => return,
                Ok(AgentStartupSignal::ProcessExited) => {
                    process_exited = true;
                    service.publish_agent_startup_report(
                        &supervisor_tab_id,
                        &agent_tab_id,
                        &agent_title,
                        agent_startup_process_exited_event(
                            &agent_tab_id,
                            &agent_title,
                            &attempt_id,
                        ),
                    );
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if !service.startup_attempt_is_current(&agent_tab_id, &attempt_id) {
                        return;
                    }
                    service.publish_agent_startup_report(
                        &supervisor_tab_id,
                        &agent_tab_id,
                        &agent_title,
                        agent_startup_notice_event(&agent_tab_id, &agent_title, &attempt_id),
                    );
                }
            }
            if !service.startup_attempt_is_current(&agent_tab_id, &attempt_id) {
                return;
            }
            loop {
                let remaining = final_deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                match receiver.recv_timeout(remaining) {
                    Ok(AgentStartupSignal::Ready | AgentStartupSignal::Held | AgentStartupSignal::Superseded) => return,
                    Ok(AgentStartupSignal::ProcessExited) if !process_exited => {
                        process_exited = true;
                        service.publish_agent_startup_report(
                            &supervisor_tab_id,
                            &agent_tab_id,
                            &agent_title,
                            agent_startup_process_exited_event(
                                &agent_tab_id,
                                &agent_title,
                                &attempt_id,
                            ),
                        );
                    }
                    Ok(AgentStartupSignal::ProcessExited) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                }
            }
            if !service.worker_ack.expire_startup(&agent_tab_id, &attempt_id)
                || !service.startup_attempt_is_current(&agent_tab_id, &attempt_id)
            {
                return;
            }
            let cleanup_error = serialize_agent_startup_cleanup(&service.worker_ack, || {
                service
                    .close_tab_with_reason(
                        &agent_tab_id,
                        crate::multiplexer::types::TabCloseReason::PolicyStartup,
                    )
                    .err()
            });
            service.publish_agent_startup_report(
                &supervisor_tab_id,
                &agent_tab_id,
                &agent_title,
                agent_startup_failed_event(
                    &agent_tab_id,
                    &agent_title,
                    &attempt_id,
                    cleanup_error.as_deref(),
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

    fn deliver_worker_ack_reminder(&self, tab_id: &str, attempt_id: &str) -> Result<(), String> {
        use crate::service::message_gateway::{
            DeliveryClass, MessageBody, MessageOrigin, OutboundMessage,
        };
        let envelope = crate::service::messenger::MessageEnvelope::codeterm_reminder("worker-ack")
            .id(Some(attempt_id.to_string()));
        self.gateway
            .submit(
                self,
                OutboundMessage {
                    target: tab_id.to_string(),
                    body: MessageBody::Text {
                        body: WORKER_ACK_REMINDER.to_string(),
                    },
                    envelope: Some(envelope),
                    class: DeliveryClass::SystemNotice,
                    origin: MessageOrigin::System,
                    idempotency_key: Some(format!("worker-ack-reminder:{attempt_id}")),
                },
            )
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
        startup_allowance: Duration,
    ) {
        let acknowledged = self.plan_store.get(plan_id).is_some_and(|plan| {
            plan.tasks
                .iter()
                .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
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
            // A queued lane setup command runs before the agent boots; the
            // first ack deadline stretches by that bound, never the final one.
            match receiver.recv_timeout(WORKER_ACK_TIMEOUT + startup_allowance) {
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
            match first_deadline_action(reminder_allowed) {
                WorkerAckFirstDeadline::SystemReminder => {
                    if let Err(error) =
                        service.deliver_worker_ack_reminder(&worker_tab_id, &attempt_id)
                    {
                        log::warn!(
                            "[worker-ack] reminder delivery failed attempt={attempt_id}: {error}"
                        );
                    }
                }
                WorkerAckFirstDeadline::StartupUnbound => {
                    let _ = service.plan_store.mark_worker_startup_deadline(
                        &plan_id,
                        &task_id,
                        &worker_tab_id,
                        &attempt_id,
                        false,
                        epoch_ms(),
                    );
                }
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
            let cleanup_error = close_worker_after_final_deadline(|| {
                service.close_tab_with_reason(
                    &worker_tab_id,
                    crate::multiplexer::types::TabCloseReason::PolicyWorker,
                )
            });
            if cleanup_error.is_some() {
                let _ = service.plan_store.mark_worker_startup_cleanup_failed(
                    &plan_id,
                    &task_id,
                    &attempt_id,
                    epoch_ms(),
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
                    cleanup_error.as_deref(),
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
            Duration::ZERO,
        );
        Ok(true)
    }
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acknowledgement_resolves_only_the_matching_attempt() {
        let registry = WorkerAckRegistry::default();
        let receiver = registry.arm("tab", "attempt-a");

        assert!(!registry.acknowledge("tab", "attempt-b"));
        assert!(registry.acknowledge("tab", "attempt-a"));
        assert_eq!(receiver.recv().unwrap(), WorkerAckSignal::Acknowledged);
    }

    #[test]
    fn rearm_supersedes_the_previous_deadline() {
        let registry = WorkerAckRegistry::default();
        let previous = registry.arm("tab", "attempt-a");
        let current = registry.arm("tab", "attempt-b");

        assert_eq!(previous.recv().unwrap(), WorkerAckSignal::Superseded);
        assert!(registry.acknowledge("tab", "attempt-b"));
        assert_eq!(current.recv().unwrap(), WorkerAckSignal::Acknowledged);
    }

    #[test]
    fn expiry_removes_only_the_matching_deadline() {
        let registry = WorkerAckRegistry::default();
        let receiver = registry.arm("tab", "attempt-a");

        assert!(!registry.expire("tab", "attempt-b"));
        assert!(registry.expire("tab", "attempt-a"));
        assert!(receiver.recv().is_err());
    }

    #[test]
    fn bound_worker_missing_ack_gets_one_system_reminder() {
        assert_eq!(
            first_deadline_action(true),
            WorkerAckFirstDeadline::SystemReminder
        );
        assert_eq!(
            WORKER_ACK_REMINDER,
            "Acknowledge this Worker assignment with codeterm agent ack before doing any work. If acknowledgement is still absent at the final bounded startup deadline, this tab will be cleaned up."
        );
        assert!(!WORKER_ACK_REMINDER.contains("Task body"));
    }

    #[test]
    fn unbound_worker_gets_no_ack_reminder_or_input() {
        assert_eq!(
            first_deadline_action(false),
            WorkerAckFirstDeadline::StartupUnbound
        );
    }

    #[test]
    fn worker_ack_final_deadline_cleans_once() {
        let registry = WorkerAckRegistry::default();
        let receiver = registry.arm("tab", "attempt");

        assert!(registry.expire("tab", "attempt"));
        assert!(!registry.expire("tab", "attempt"));
        assert!(receiver.recv().is_err());
    }

    #[test]
    fn worker_ack_close_tab_failure_is_a_coordination_incident() {
        let cleanup_error = close_worker_after_final_deadline(|| Err("tab remained open".into()));
        let event = worker_ack_timeout_event(
            "plan",
            "task",
            "tab",
            "attempt",
            "codeterm plan task dispatch plan task",
            cleanup_error.as_deref(),
        );

        assert_eq!(cleanup_error.as_deref(), Some("tab remained open"));
        assert_eq!(event["kind"], "worker_ack_cleanup_failed");
        assert_eq!(event["cleanup"]["ok"], false);
        assert!(event["impact"]
            .as_str()
            .unwrap()
            .contains("ORCHESTRATION PRODUCT INCIDENT"));
        assert!(event["fallback"].as_str().unwrap().contains("still live"));
    }

    #[test]
    fn startup_acknowledgement_and_hold_resolve_only_the_current_attempt() {
        let registry = WorkerAckRegistry::default();
        let ready = registry.arm_startup("tab-a", "launch-a");
        assert!(!registry.mark_startup_ready("tab-a", "launch-old"));
        assert!(registry.mark_startup_ready("tab-a", "launch-a"));
        assert_eq!(ready.recv().unwrap(), AgentStartupSignal::Ready);

        let held = registry.arm_startup("tab-a", "launch-b");
        assert!(!registry.hold_startup("tab-a", "launch-old"));
        assert!(registry.hold_startup("tab-a", "launch-b"));
        assert_eq!(held.recv().unwrap(), AgentStartupSignal::Held);

        let exited = registry.arm_startup("tab-a", "launch-c");
        assert!(!registry.mark_startup_process_exited("tab-a", "launch-old"));
        assert!(registry.mark_startup_process_exited("tab-a", "launch-c"));
        assert_eq!(exited.recv().unwrap(), AgentStartupSignal::ProcessExited);
        assert!(registry.hold_startup("tab-a", "launch-c"));
        assert_eq!(exited.recv().unwrap(), AgentStartupSignal::Held);
    }

    #[test]
    fn startup_ack_requires_the_current_pty_and_launch_nonce() {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.pty_id = Some("pty-current".into());
        tab.session_binding_nonce = Some("nonce-current".into());

        assert!(startup_ack_owns_attempt(
            tab,
            "pty-current",
            "nonce-current"
        ));
        assert!(!startup_ack_owns_attempt(
            tab,
            "pty-stale",
            "nonce-current"
        ));
        assert!(!startup_ack_owns_attempt(
            tab,
            "pty-current",
            "nonce-stale"
        ));
    }

    #[test]
    fn startup_supervisor_registration_is_consumed_once() {
        let registry = WorkerAckRegistry::default();
        registry.register_startup_supervisor("tab-a", "parent", "Planner");

        let supervisor = registry.take_startup_supervisor("tab-a").unwrap();
        assert_eq!(supervisor.supervisor_tab_id, "parent");
        assert_eq!(supervisor.agent_title, "Planner");
        assert!(registry.take_startup_supervisor("tab-a").is_none());
    }

    #[test]
    fn startup_notice_exposes_inspect_respond_and_hold_without_task_resend() {
        let event = agent_startup_notice_event("child", "Planner", "launch-a");
        assert_eq!(event["kind"], "agent_startup_overdue");
        assert_eq!(event["deadline_seconds"], 30);
        assert_eq!(
            event["hold"],
            "codeterm agent startup-hold --tab child --attempt launch-a"
        );
        assert!(event["inspect"].as_array().unwrap().iter().any(|command| {
            command
                .as_str()
                .is_some_and(|command| command.contains("tab get-text"))
        }));
        assert!(!event.to_string().contains("send the task"));
    }

    #[test]
    fn exited_startup_is_cleaned_at_one_minute() {
        let event = agent_startup_failed_event("child", "Planner", "launch-a", None);
        assert_eq!(event["kind"], "agent_startup_failed");
        assert_eq!(event["deadline_seconds"], 60);
        assert_eq!(event["cleanup"]["ok"], true);
    }

    #[test]
    fn startup_cleanup_holds_the_gate_across_tab_close() {
        let registry = Arc::new(WorkerAckRegistry::default());
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let cleanup_registry = registry.clone();
        let cleanup = std::thread::spawn(move || {
            serialize_agent_startup_cleanup(&cleanup_registry, || {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            });
        });
        entered_rx.recv().unwrap();
        assert!(registry.startup_cleanup.try_lock().is_none());
        release_tx.send(()).unwrap();
        cleanup.join().unwrap();
        assert!(registry.startup_cleanup.try_lock().is_some());
    }
}
