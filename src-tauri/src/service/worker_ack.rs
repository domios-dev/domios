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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkerAckSignal {
    Acknowledged,
    Superseded,
}

struct WorkerAckWaiter {
    attempt_id: String,
    sender: Sender<WorkerAckSignal>,
}

#[derive(Default)]
pub(crate) struct WorkerAckRegistry {
    waiters: Mutex<HashMap<String, WorkerAckWaiter>>,
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

impl ServiceContext {
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
        let matches = self
            .plan_store
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
                            .map(|attempt| (plan_id.clone(), task.id, attempt.clone()))
                    })
            })
            .collect::<Vec<_>>();
        let [(plan_id, task_id, attempt)] = matches.as_slice() else {
            if matches.is_empty() {
                return Ok(false);
            }
            return Err(format!(
                "expected one live task attempt for tab {tab_id}, found {}",
                matches.len()
            ));
        };
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
}
