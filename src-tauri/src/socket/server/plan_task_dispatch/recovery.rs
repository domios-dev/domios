use serde_json::{json, Value};

use crate::agent::reports::ReportDetails;
use crate::service::message_gateway::{DeliveryClass, MessageBody, MessageOrigin, OutboundMessage};
use crate::service::messenger::MessageEnvelope;
use crate::service::ServiceState;

pub(super) fn startup_recovery_event(
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    attempt_id: &str,
    startup_attempt_id: Option<&str>,
    startup_stage: &str,
) -> Value {
    let mut event = json!({
        "kind": "worker_startup_recovery_needed",
        "action_needed": true,
        "state": "blocked_pending_parent_inspection",
        "trigger": "uncertain_startup_readiness",
        "plan_id": plan_id,
        "task_id": task_id,
        "tab_id": tab_id,
        "attempt_id": attempt_id,
        "startup_stage": startup_stage,
        "inspect": {
            "lifecycle": format!("codeterm tab inspect --tab {tab_id} --lifecycle"),
            "screen": format!("codeterm tab get-text --tab {tab_id}"),
        },
        "prompt_classification": {
            "source": "declared_provider_classifier",
            "confirm_with": "current_screen",
        },
        "respond": {
            "command": format!("codeterm tab respond OPTION --tab {tab_id}"),
            "requires_recognized_safe_prompt": true,
        },
        "verification": {
            "reinspect_lifecycle": true,
            "reinspect_current_screen": true,
            "authoritative_session_binding": true,
            "durable_worker_ack": true,
        },
        "unknown_prompt": {
            "state": "blocked",
            "action_needed": true,
            "respond": false,
        },
        "task_prose_to_worker": false,
        "ack_reminder_to_worker": false,
        "raw_keys": false,
        "auto_accept": false,
    });
    if let Some(startup_attempt_id) = startup_attempt_id.filter(|id| !id.trim().is_empty()) {
        event["startup_attempt_id"] = json!(startup_attempt_id);
        event["startup_hold"] = json!({
            "command": format!(
                "codeterm agent startup-hold --tab {tab_id} --attempt {startup_attempt_id}"
            ),
            "requires_prompt_bound_evidence": true,
        });
    }
    event
}

pub(super) fn publish_action_needed_event(
    svc: &ServiceState,
    parent_tab_id: &str,
    worker_tab_id: &str,
    worker_title: &str,
    payload: Value,
) {
    let message = serde_json::to_string(&payload).unwrap_or_else(|_| {
        json!({
            "kind": "worker_recovery_event_serialization_failed",
            "tab_id": worker_tab_id,
        })
        .to_string()
    });
    let report = svc.report_store.lock().add_detailed(
        parent_tab_id,
        worker_tab_id,
        worker_title,
        &message,
        ReportDetails::default(),
        None,
    );
    if !svc.report_store.lock().claim(&report.id) {
        return;
    }
    svc.events.broadcast(
        "agent.report",
        json!({
            "target_tab_id": parent_tab_id,
            "report": report.clone(),
        }),
    );
    crate::api::events::emit_everywhere(
        &svc.app_handle,
        "agent-report",
        json!({
            "target_tab_id": parent_tab_id,
            "from_tab_id": worker_tab_id,
            "from_title": worker_title,
            "message": message,
            "report": report.clone(),
        }),
    );
    let message = OutboundMessage {
        target: parent_tab_id.to_string(),
        body: MessageBody::Text {
            body: report.inject_body(),
        },
        envelope: Some(MessageEnvelope::report(worker_title, worker_tab_id)),
        class: DeliveryClass::AgentTraffic,
        origin: MessageOrigin::System,
        idempotency_key: Some(format!("worker-recovery-event:{}", report.id)),
    };
    if svc.gateway.submit(svc.as_ref(), message).is_ok() {
        svc.report_store.lock().mark_delivered(&report.id);
    } else {
        svc.report_store.lock().release_delivery(&report.id);
    }
}

#[cfg(test)]
#[path = "recovery/tests.rs"]
mod tests;
