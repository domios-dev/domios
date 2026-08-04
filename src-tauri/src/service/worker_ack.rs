use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Value};

use crate::agent::reports::ReportDetails;

use super::ServiceContext;

const WORKER_ACK_TIMEOUT: Duration = Duration::from_secs(60);

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
) -> Value {
    json!({
        "kind": "worker_ack_timeout",
        "plan_id": plan_id,
        "task_id": task_id,
        "tab_id": tab_id,
        "attempt_id": attempt_id,
        "deadline_seconds": WORKER_ACK_TIMEOUT.as_secs(),
        "expected_command": "codeterm agent ack",
        "spawn_command": spawn_command,
        "impact": "Worker ownership is unconfirmed; task remains awaiting_worker_ack.",
        "fallback": "Inspect the existing tab once. Do not blind-resend or duplicate-dispatch.",
    })
}

impl ServiceContext {
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
            let acknowledged = service.plan_store.get(&plan_id).is_some_and(|plan| {
                plan.tasks
                    .iter()
                    .find(|task| task.id == task_id)
                    .into_iter()
                    .flat_map(|task| task.execution_attempts.iter())
                    .any(|attempt| attempt.id == attempt_id && attempt.acknowledged_at.is_some())
            });
            if !service.worker_ack.expire(&worker_tab_id, &attempt_id) || acknowledged {
                return;
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
}
