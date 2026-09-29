use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

use crate::plan::{Plan, PlanStatus, Task, TaskStatus};
use crate::service::message_gateway::{DeliveryClass, MessageBody, MessageOrigin, OutboundMessage};
use crate::service::messenger::MessageEnvelope;
use crate::service::ServiceContext;

pub(crate) const WORKER_LOST_EVENT: &str = "worker-lost";
pub(crate) const WORKER_LOST_CONTEXT: &str = "worker-lost";
pub(crate) const BUSY_WORKER_CLOSE_REFUSAL_PREFIX: &str = "busy worker close refused";

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkerLossCause {
    TabGone,
    ProcessExited,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(crate) enum LaneState {
    NoLane,
    Missing,
    Clean,
    Dirty { changes: usize },
    Unknown { error: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct WorkerLoss {
    pub plan_id: String,
    pub task_id: String,
    pub task_name: String,
    pub attempt_id: Option<String>,
    pub worker_tab_id: String,
    pub orchestrator_tab_id: Option<String>,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    pub lane: LaneState,
    pub cause: WorkerLossCause,
    pub recovery: String,
}

impl WorkerLoss {
    fn dedupe_key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.plan_id,
            self.task_id,
            self.attempt_id.as_deref().unwrap_or(&self.worker_tab_id)
        )
    }

    fn lane_summary(&self) -> String {
        let state = match &self.lane {
            LaneState::NoLane => "no lane recorded".to_string(),
            LaneState::Missing => "worktree missing".to_string(),
            LaneState::Clean => "clean".to_string(),
            LaneState::Dirty { changes } => format!("dirty ({changes} uncommitted change(s))"),
            LaneState::Unknown { error } => format!("unknown ({error})"),
        };
        format!(
            "worktree={} branch={} state={state}",
            self.worktree_path.as_deref().unwrap_or("<none>"),
            self.branch.as_deref().unwrap_or("<none>")
        )
    }

    pub(crate) fn notice_text(&self) -> String {
        let cause = match self.cause {
            WorkerLossCause::TabGone => "its tab is gone",
            WorkerLossCause::ProcessExited => "its agent process exited",
        };
        format!(
            "[worker lost: {task}] Worker {tab} stopped ({cause}) while task {task} \"{name}\" of plan {plan} was in_progress.\n\
             lane: {lane}\n\
             recover: {recovery}",
            task = self.task_id,
            tab = self.worker_tab_id,
            name = self.task_name,
            plan = self.plan_id,
            lane = self.lane_summary(),
            recovery = self.recovery,
        )
    }

    pub(crate) fn refusal_line(&self) -> String {
        format!(
            "task {} \"{}\" of plan {} is in_progress on this tab ({})",
            self.task_id,
            self.task_name,
            self.plan_id,
            self.lane_summary()
        )
    }

    pub(crate) fn outbound_message(&self, target: &str) -> OutboundMessage {
        OutboundMessage {
            target: target.to_string(),
            body: MessageBody::Text {
                body: self.notice_text(),
            },
            envelope: Some(MessageEnvelope::codeterm_reminder(WORKER_LOST_CONTEXT)),
            class: DeliveryClass::SystemNotice,
            origin: MessageOrigin::System,
            idempotency_key: Some(format!("worker-lost:{}", self.dedupe_key())),
        }
    }
}

pub(crate) fn busy_worker_close_warnings(tab_id: &str, assignments: &[WorkerLoss]) -> Vec<String> {
    assignments
        .iter()
        .map(|loss| {
            format!(
                "closed Worker {tab_id} with unfinished work: {}; its Orchestrator is notified; recover with `{}`",
                loss.refusal_line(),
                loss.recovery
            )
        })
        .collect()
}

pub(crate) fn recovery_command(plan_id: &str, task_id: &str, attempt_id: Option<&str>) -> String {
    match attempt_id {
        Some(attempt_id) => {
            format!("codeterm plan task respawn {plan_id} {task_id} --attempt {attempt_id}")
        }
        None => format!("codeterm plan reconcile {plan_id} --fix"),
    }
}

fn bound_to_tab(task: &Task, tab_id: &str) -> bool {
    task.assigned_tab_id.as_deref() == Some(tab_id)
        || task.lane.tab_id.as_deref() == Some(tab_id)
        || task
            .execution_attempts
            .iter()
            .any(|attempt| attempt.closed_at.is_none() && attempt.executor_tab_id == tab_id)
}

fn loss_from_task(plan: &Plan, task: &Task, tab_id: &str, cause: WorkerLossCause) -> WorkerLoss {
    let attempt = task
        .execution_attempts
        .iter()
        .rev()
        .find(|attempt| attempt.executor_tab_id == tab_id);
    let worktree_path = attempt
        .and_then(|attempt| attempt.worktree_path.clone())
        .or_else(|| task.lane.worktree_path.clone())
        .filter(|path| !path.trim().is_empty());
    let branch = attempt
        .and_then(|attempt| attempt.branch.clone())
        .or_else(|| task.lane.branch.clone())
        .filter(|branch| !branch.trim().is_empty());
    let attempt_id = attempt.map(|attempt| attempt.id.clone());
    WorkerLoss {
        plan_id: plan.id.clone(),
        task_id: task.id.clone(),
        task_name: task.name.clone(),
        recovery: recovery_command(&plan.id, &task.id, attempt_id.as_deref()),
        attempt_id,
        worker_tab_id: tab_id.to_string(),
        orchestrator_tab_id: plan.owner.tab_id.clone().filter(|owner| owner != tab_id),
        worktree_path,
        branch,
        lane: LaneState::NoLane,
        cause,
    }
}

/// In-progress tasks of active plans that `tab_id` executes; lanes unprobed.
pub(crate) fn in_progress_losses(
    plans: &[Plan],
    tab_id: &str,
    cause: WorkerLossCause,
) -> Vec<WorkerLoss> {
    let tab_id = tab_id.trim();
    if tab_id.is_empty() {
        return Vec::new();
    }
    plans
        .iter()
        .filter(|plan| plan.status == PlanStatus::Active)
        .flat_map(|plan| {
            plan.tasks
                .iter()
                .filter(|task| task.status == TaskStatus::InProgress)
                .filter(|task| bound_to_tab(task, tab_id))
                .map(move |task| loss_from_task(plan, task, tab_id, cause))
        })
        .collect()
}

pub(crate) fn probe_lane_state(worktree_path: Option<&str>) -> LaneState {
    let Some(path) = worktree_path else {
        return LaneState::NoLane;
    };
    if !Path::new(path).exists() {
        return LaneState::Missing;
    }
    let mut command = Command::new("git");
    crate::process_util::no_window_std(&mut command);
    let output = command
        .current_dir(path)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output();
    match output {
        Err(error) => LaneState::Unknown {
            error: format!("git could not run: {error}"),
        },
        Ok(output) if !output.status.success() => LaneState::Unknown {
            error: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        },
        Ok(output) => {
            let changes = String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count();
            if changes == 0 {
                LaneState::Clean
            } else {
                LaneState::Dirty { changes }
            }
        }
    }
}

fn reported_losses() -> &'static Mutex<HashSet<String>> {
    static REPORTED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    REPORTED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn claim_report(loss: &WorkerLoss) -> bool {
    reported_losses()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(loss.dedupe_key())
}

impl ServiceContext {
    pub(crate) fn in_progress_assignments_for_tab(&self, tab_id: &str) -> Vec<WorkerLoss> {
        in_progress_losses(
            &self.plan_store.list_full(),
            tab_id,
            WorkerLossCause::TabGone,
        )
    }

    pub(crate) fn worker_losses_for_tab(
        &self,
        tab_id: &str,
        cause: WorkerLossCause,
    ) -> Vec<WorkerLoss> {
        let mut losses = in_progress_losses(&self.plan_store.list_full(), tab_id, cause);
        for loss in &mut losses {
            loss.lane = probe_lane_state(loss.worktree_path.as_deref());
        }
        losses
    }

    pub(crate) fn worker_loss_for_task(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        cause: WorkerLossCause,
    ) -> Option<WorkerLoss> {
        let plan = self.plan_store.get(plan_id)?;
        let task = plan.tasks.iter().find(|task| task.id == task_id)?;
        let mut loss = loss_from_task(&plan, task, tab_id, cause);
        loss.lane = probe_lane_state(loss.worktree_path.as_deref());
        Some(loss)
    }

    /// Pushes each loss once: an Orchestrator notice through the gateway and a
    /// `worker-lost` bus event carrying the structured record and its outcome.
    pub(crate) fn report_worker_losses(&self, losses: Vec<WorkerLoss>) {
        for loss in losses {
            if !claim_report(&loss) {
                continue;
            }
            let orchestrator = loss
                .orchestrator_tab_id
                .as_deref()
                .filter(|owner| self.mux.read().get_tab(owner).is_some());
            let delivery = match orchestrator {
                None => "no_live_orchestrator".to_string(),
                Some(owner) => match self.gateway.submit(self, loss.outbound_message(owner)) {
                    Ok(_) => "delivered".to_string(),
                    Err(error) => format!("failed: {error}"),
                },
            };
            log::warn!(
                "[worker-lost] plan={} task={} worker={} orchestrator={} cause={:?} delivery={}",
                loss.plan_id,
                loss.task_id,
                loss.worker_tab_id,
                loss.orchestrator_tab_id.as_deref().unwrap_or("none"),
                loss.cause,
                delivery
            );
            let mut payload = serde_json::to_value(&loss).unwrap_or_default();
            payload["delivery"] = serde_json::Value::String(delivery);
            self.events.publish_generic(WORKER_LOST_EVENT, payload);
        }
    }
}
