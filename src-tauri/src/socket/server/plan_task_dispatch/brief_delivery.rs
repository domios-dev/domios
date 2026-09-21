use std::collections::HashMap;
use std::sync::LazyLock;

use parking_lot::Mutex;
use serde_json::json;

use crate::plan::{TaskExecutionAttempt, TaskStatus};
use crate::service::message_gateway::{DeliveryClass, MessageBody, MessageOrigin, OutboundMessage};
use crate::service::messenger::MessageEnvelope;
use crate::service::ServiceState;

#[derive(Clone)]
struct DeferredTaskBrief {
    plan_id: String,
    task_id: String,
    tab_id: Option<String>,
    body: Option<String>,
    claimed: bool,
}

static DEFERRED_TASK_BRIEFS: LazyLock<Mutex<HashMap<String, DeferredTaskBrief>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TaskBriefDelivery {
    NotReady,
    Submitted,
    AlreadySubmitted,
    Failed(String),
}

pub(super) fn defer_task_brief_before_launch(
    plan_id: &str,
    task_id: &str,
    attempt_id: &str,
    body: String,
) -> Result<Option<String>, String> {
    let mut deferred = DEFERRED_TASK_BRIEFS.lock();
    if deferred.contains_key(attempt_id) {
        return Err(format!(
            "a deferred brief already exists for attempt {attempt_id}"
        ));
    }
    deferred.insert(
        attempt_id.to_string(),
        DeferredTaskBrief {
            plan_id: plan_id.to_string(),
            task_id: task_id.to_string(),
            tab_id: None,
            body: Some(body),
            claimed: false,
        },
    );
    Ok(None)
}

pub(super) fn bind_deferred_task_brief_to_tab(
    attempt_id: &str,
    tab_id: &str,
) -> Result<(), String> {
    let mut deferred = DEFERRED_TASK_BRIEFS.lock();
    let Some(brief) = deferred.get_mut(attempt_id) else {
        return Err(format!(
            "deferred brief is missing for attempt {attempt_id}"
        ));
    };
    if brief.tab_id.as_deref().is_some_and(|bound| bound != tab_id) {
        return Err(format!(
            "deferred brief attempt {attempt_id} is already bound to another tab"
        ));
    }
    brief.tab_id = Some(tab_id.to_string());
    Ok(())
}

pub(super) fn discard_deferred_task_brief(attempt_id: &str) {
    DEFERRED_TASK_BRIEFS.lock().remove(attempt_id);
}

pub(super) fn task_brief_is_ready(
    task_status: &TaskStatus,
    assigned_tab_id: Option<&str>,
    attempt: &TaskExecutionAttempt,
    tab_id: &str,
) -> bool {
    *task_status == TaskStatus::InProgress
        && assigned_tab_id == Some(tab_id)
        && attempt.executor_tab_id == tab_id
        && attempt.session_id.as_deref().is_some_and(non_empty)
        && attempt
            .session_binding_nonce
            .as_deref()
            .is_some_and(non_empty)
        && attempt.acknowledged_at.is_some()
        && attempt.closed_at.is_none()
        && attempt.retired_at.is_none()
}

fn non_empty(value: &str) -> bool {
    !value.trim().is_empty()
}

pub(crate) fn deliver_deferred_task_brief(
    svc: &ServiceState,
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    attempt_id: &str,
) -> TaskBriefDelivery {
    let Some(detail) = svc.plan_store.show_task(task_id) else {
        return TaskBriefDelivery::Failed(format!(
            "task {task_id} disappeared before brief delivery"
        ));
    };
    if detail.plan_id != plan_id {
        return TaskBriefDelivery::Failed(format!(
            "task {task_id} resolved to plan {} instead of {plan_id}",
            detail.plan_id
        ));
    }
    let Some(attempt) = detail
        .task
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == attempt_id)
    else {
        return TaskBriefDelivery::Failed(format!(
            "attempt {attempt_id} disappeared before brief delivery"
        ));
    };
    if !task_brief_is_ready(
        &detail.task.status,
        detail.task.assigned_tab_id.as_deref(),
        attempt,
        tab_id,
    ) {
        return TaskBriefDelivery::NotReady;
    }

    let (body, parent_tab_id) =
        match claim_deferred_task_brief(attempt_id, plan_id, task_id, tab_id, attempt) {
            Ok(Some(claimed)) => claimed,
            Ok(None) => return TaskBriefDelivery::AlreadySubmitted,
            Err(outcome) => return outcome,
        };

    let from = parent_tab_id
        .as_deref()
        .and_then(|parent| {
            let mux = svc.mux.read();
            mux.get_tab(parent)
                .map(|_| MessageEnvelope::tab(mux.display_label_for_tab(parent), parent))
        })
        .unwrap_or_else(MessageEnvelope::from_codeterm);
    let message = OutboundMessage {
        target: tab_id.to_string(),
        body: MessageBody::Text { body },
        envelope: Some(from),
        class: DeliveryClass::AgentTraffic,
        origin: MessageOrigin::System,
        idempotency_key: Some(format!("task-brief:{plan_id}:{task_id}:{attempt_id}")),
    };
    match svc.gateway.submit(svc.as_ref(), message) {
        Ok(_) => {
            svc.gateway.require_worker_report(tab_id);
            TaskBriefDelivery::Submitted
        }
        Err(error) => TaskBriefDelivery::Failed(error.to_string()),
    }
}

fn claim_deferred_task_brief(
    attempt_id: &str,
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    attempt: &TaskExecutionAttempt,
) -> Result<Option<(String, Option<String>)>, TaskBriefDelivery> {
    let mut deferred = DEFERRED_TASK_BRIEFS.lock();
    let Some(brief) = deferred.get_mut(attempt_id) else {
        return Err(TaskBriefDelivery::Failed(format!(
            "deferred brief snapshot is missing for ready attempt {attempt_id}"
        )));
    };
    if brief.plan_id != plan_id
        || brief.task_id != task_id
        || brief.tab_id.as_deref() != Some(tab_id)
    {
        return Err(TaskBriefDelivery::Failed(format!(
            "deferred brief ownership does not match ready attempt {attempt_id}"
        )));
    }
    if brief.claimed {
        return Ok(None);
    }
    let Some(body) = brief.body.take() else {
        return Err(TaskBriefDelivery::Failed(format!(
            "deferred brief payload is unavailable for attempt {attempt_id}"
        )));
    };
    brief.claimed = true;
    Ok(Some((
        body,
        attempt
            .orchestrator_tab_id
            .as_deref()
            .or(attempt.parent_tab_id.as_deref())
            .map(str::to_string),
    )))
}

pub(crate) fn task_brief_delivery_event(
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    attempt_id: &str,
    error: &str,
) -> serde_json::Value {
    json!({
        "kind": "worker_task_brief_delivery_action_needed",
        "action_needed": true,
        "state": "blocked",
        "plan_id": plan_id,
        "task_id": task_id,
        "tab_id": tab_id,
        "attempt_id": attempt_id,
        "impact": "The task ACK and session binding are durable, but the actionable brief was not submitted.",
        "error": error,
        "next_action": "Inspect this exact tab and lifecycle before choosing a supported recovery; do not resend blindly.",
        "task_prose_to_worker": false,
    })
}

#[cfg(test)]
#[path = "brief_delivery/tests.rs"]
mod tests;
