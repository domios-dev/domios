use serde_json::{json, Value};

use super::{
    NumberedPromptOption, ObservationTrigger, SignalAction, TabStateMetadata, CRASHED_TO_SHELL,
    ERROR_LOOP, LOGIN_OR_AUTH_REQUIRED, MODEL_UNAVAILABLE_OR_QUOTA, PROVIDER_UPDATE_PROMPT,
    READY_IDLE, SPAWN_OK_LINE, STILL_STARTING, TRUST_OR_PERMISSION_PROMPT, WEAK_HINT_PREFIX,
    WORKING_NORMALLY,
};
use crate::service::message_gateway::DeliveryClass;
use crate::service::messenger::MessageEnvelope;
use crate::service::ServiceContext;

const PROMPT_RESPONSE_GUIDANCE: &str =
    "Resolve its pending choice with `codeterm tab respond <number> --tab {tab_id}`.";
const AUTH_SIGNAL_GUIDANCE: &str = "needs authentication or first-run setup.";
const AUTH_SIGNAL_EVENT_TYPE: &str = "tab_state_auth_required";
const MODEL_UNAVAILABLE_GUIDANCE: &str =
    "cannot use its requested model. Respawn it with a model from the provider catalogue.";
const CRASH_RECOVERY_GUIDANCE: &str =
    "returned to its shell. Inspect with `codeterm tab get-text --tab {tab_id}` and recover with `codeterm agent restart --tab {tab_id}` or respawn it.";
const ERROR_LOOP_RECOVERY_GUIDANCE: &str =
    "is repeating an error. Inspect with `codeterm tab get-text --tab {tab_id}` and recover with `codeterm agent restart --tab {tab_id}` or respawn it.";
const STILL_STARTING_GUIDANCE: &str = "is still starting after its startup deadline.";

#[cfg(test)]
#[path = "delivery_tests.rs"]
mod tests;

pub(super) fn deliver(
    service: &ServiceContext,
    tab_id: &str,
    metadata: &TabStateMetadata,
    episode: u64,
    trigger: ObservationTrigger,
    label: &str,
    confidence: f64,
    action: SignalAction,
) {
    match action {
        SignalAction::SpawnOk => publish_spawn_ok(service, tab_id, metadata, episode),
        SignalAction::WeakHint => {
            publish_weak_hint(service, tab_id, metadata, label, confidence, episode)
        }
        SignalAction::StrongAction => publish_event(
            service, tab_id, metadata, trigger, label, confidence, episode,
        ),
        SignalAction::None => {}
    }
}

fn publish_spawn_ok(
    service: &ServiceContext,
    tab_id: &str,
    metadata: &TabStateMetadata,
    episode: u64,
) {
    let payload = json!({
        "kind": "tab_state_spawn_ok",
        "tab_id": tab_id,
        "task_id": metadata.task_id,
        "attempt_id": metadata.attempt_id,
        "episode": episode,
        "message": SPAWN_OK_LINE,
    });
    if let (Some(target), Some(plan_id), Some(task_id)) = (
        orchestrator_target(service, metadata),
        metadata.plan_id.as_deref(),
        metadata.task_id.as_deref(),
    ) {
        service.publish_worker_lifecycle_report(
            &target,
            tab_id,
            &metadata.title,
            plan_id,
            task_id,
            payload,
        );
        return;
    }
    service.events.publish_generic("tab-state-signal", payload);
    if let Some(target) = metadata
        .spawning_tab_id
        .as_deref()
        .or(metadata.parent_tab_id.as_deref())
    {
        deliver_to_target(service, target, SPAWN_OK_LINE);
    }
}

fn publish_weak_hint(
    service: &ServiceContext,
    tab_id: &str,
    metadata: &TabStateMetadata,
    label: &str,
    confidence: f64,
    episode: u64,
) {
    let line = format!(
        "{WEAK_HINT_PREFIX}: tab={tab_id} label={label} confidence={confidence:.2} episode={episode}"
    );
    service.events.publish_generic(
        "tab-state-signal",
        json!({
            "tab_id": tab_id,
            "label": label,
            "confidence": confidence,
            "band": "weak",
            "episode": episode,
        }),
    );
    if let Some(target) = orchestrator_target(service, metadata) {
        deliver_to_target(service, &target, &line);
    }
}

fn publish_event(
    service: &ServiceContext,
    tab_id: &str,
    metadata: &TabStateMetadata,
    trigger: ObservationTrigger,
    label: &str,
    confidence: f64,
    episode: u64,
) {
    if matches!(label, READY_IDLE | WORKING_NORMALLY) {
        return;
    }
    let options = numbered_options(&metadata.prompt_options);
    let (event_type, message) = match label {
        TRUST_OR_PERMISSION_PROMPT | PROVIDER_UPDATE_PROMPT => (
            "tab_state_prompt_required",
            format!(
                "Tab {tab_id} is blocked on {label}. {}{}",
                PROMPT_RESPONSE_GUIDANCE.replace("{tab_id}", tab_id),
                options
            ),
        ),
        LOGIN_OR_AUTH_REQUIRED => (
            AUTH_SIGNAL_EVENT_TYPE,
            format!("Tab {tab_id} {AUTH_SIGNAL_GUIDANCE}"),
        ),
        MODEL_UNAVAILABLE_OR_QUOTA => (
            "tab_state_model_unavailable",
            format!("Tab {tab_id} {MODEL_UNAVAILABLE_GUIDANCE}"),
        ),
        CRASHED_TO_SHELL => (
            "tab_state_crashed_to_shell",
            format!(
                "Tab {tab_id} {}",
                CRASH_RECOVERY_GUIDANCE.replace("{tab_id}", tab_id)
            ),
        ),
        ERROR_LOOP => (
            "tab_state_error_loop",
            format!(
                "Tab {tab_id} {}",
                ERROR_LOOP_RECOVERY_GUIDANCE.replace("{tab_id}", tab_id)
            ),
        ),
        STILL_STARTING if trigger.after_startup_deadline() => (
            "tab_state_still_starting",
            format!("Tab {tab_id} {STILL_STARTING_GUIDANCE}"),
        ),
        _ => return,
    };
    let payload = json!({
        "tab_id": tab_id,
        "label": label,
        "confidence": confidence,
        "band": "strong",
        "event": trigger.label(),
        "episode": episode,
        "pending_options": metadata.prompt_options.iter().map(|option| json!({
            "number": option.number,
            "label": option.label,
            "value": option.value,
        })).collect::<Vec<Value>>(),
    });
    service.events.publish_generic(event_type, payload);
    if let Some(target) = orchestrator_target(service, metadata) {
        deliver_to_target(service, &target, &message);
    }
}

fn numbered_options(options: &[NumberedPromptOption]) -> String {
    if options.is_empty() {
        return String::new();
    }
    let choices = options
        .iter()
        .map(|option| format!(" {}: {}", option.number, option.label))
        .collect::<Vec<_>>()
        .join(";");
    format!(" Pending options:{choices}.")
}

fn orchestrator_target(service: &ServiceContext, metadata: &TabStateMetadata) -> Option<String> {
    metadata
        .plan_id
        .as_deref()
        .and_then(|plan_id| service.plan_store.get(plan_id))
        .and_then(|plan| plan.owner.tab_id)
        .or_else(|| metadata.parent_tab_id.clone())
        .or_else(|| metadata.spawning_tab_id.clone())
}

fn deliver_to_target(service: &ServiceContext, target: &str, message: &str) {
    if let Err(error) = service.gateway_deliver(
        target,
        MessageEnvelope::codeterm_reminder("tab-state"),
        message,
        DeliveryClass::SystemNotice,
    ) {
        log::warn!("[tab-state] delivery failed target={target}: {error}");
    }
}
