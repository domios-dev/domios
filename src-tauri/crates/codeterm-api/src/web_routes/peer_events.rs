use super::EventsStreamQuery;
use crate::{auth::CallerIdentity, events::SystemEvent, routes::AppState};
use axum::http::StatusCode;
use codeterm_service_core::{
    api_support::access_policy::{allows_full_dispatch, PeerOperation, PeerResource, PeerScope},
    mesh::types::AccessLevel,
};
use serde::Deserialize;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PeerEventScope {
    Peer,
    Tab,
    Notifications,
    Template,
}

pub(super) fn validate_scope(
    state: &AppState,
    query: &EventsStreamQuery,
    caller: Option<&CallerIdentity>,
) -> Result<(), (StatusCode, String)> {
    let Some(scope) = query.scope else {
        return if matches!(caller, Some(CallerIdentity::Peer { .. })) {
            Err((
                StatusCode::FORBIDDEN,
                "Authenticated peer events require a scope".into(),
            ))
        } else {
            Ok(())
        };
    };
    let tab = query.tab_id.as_deref().filter(|tab| !tab.is_empty());
    let allowed = match (scope, tab) {
        (PeerEventScope::Tab, Some(tab)) => [PeerOperation::ReadTabs, PeerOperation::ReadOutput]
            .iter()
            .any(|operation| {
                authorized(
                    state,
                    caller,
                    PeerScope::new(*operation, PeerResource::Tab(tab)),
                )
            }),
        (PeerEventScope::Peer, None) => authorized(
            state,
            caller,
            PeerScope::new(PeerOperation::Rail, PeerResource::Host),
        ),
        (PeerEventScope::Notifications, None) => authorized(
            state,
            caller,
            PeerScope::new(PeerOperation::ReadHost, PeerResource::Host),
        ),
        (PeerEventScope::Template, None) => matches!(caller,
            Some(CallerIdentity::Peer { access_level, .. }) if allows_full_dispatch(access_level)
        ),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "Event scope does not match its resource".into(),
            ))
        }
    };
    if !allowed {
        return Err((
            StatusCode::FORBIDDEN,
            "Peer is not authorized for this event scope".into(),
        ));
    }
    Ok(())
}

pub(super) fn visible(
    state: &AppState,
    caller: Option<&CallerIdentity>,
    scope: Option<PeerEventScope>,
    tab: Option<&str>,
    event: &SystemEvent,
) -> bool {
    use PeerOperation::ReadHost;
    let scope = match scope {
        Some(PeerEventScope::Peer) => topic_scope(event),
        Some(PeerEventScope::Tab) => tab.and_then(|tab| tab_topic(event, tab)),
        Some(PeerEventScope::Notifications) => match event {
            SystemEvent::Generic { event_type, .. }
                if matches!(
                    event_type.as_str(),
                    "notification-created"
                        | "notification-updated"
                        | "notification-answered"
                        | "notification-resolved"
                        | "notification-cleared"
                ) =>
            {
                Some(PeerScope::new(ReadHost, PeerResource::Host))
            }
            _ => None,
        },
        Some(PeerEventScope::Template) => match event {
            SystemEvent::Generic { event_type, .. } if event_type == "notification-answered" => {
                Some(PeerScope::new(
                    PeerOperation::FullAccess,
                    PeerResource::Host,
                ))
            }
            _ => None,
        },
        None => return !matches!(caller, Some(CallerIdentity::Peer { .. })),
    };
    scope.is_some_and(|scope| authorized(state, caller, scope))
}

fn authorized(state: &AppState, caller: Option<&CallerIdentity>, scope: PeerScope<'_>) -> bool {
    let Some(CallerIdentity::Peer {
        access_level,
        instance_id,
        ..
    }) = caller
    else {
        return scope.allowed(&AccessLevel::Full);
    };
    scope.allowed_with_owner(access_level, |id| {
        state
            .svc
            .mux
            .read()
            .get_tab(id)
            .is_some_and(|tab| crate::auth::peer_owns_tab(instance_id, tab))
    })
}

fn topic_scope(event: &SystemEvent) -> Option<PeerScope<'_>> {
    use PeerOperation::{ReadHost, ReadMux, ReadOutput, ReadPlan, ReadTabs};
    if let Some(tab) = chat_send_status_tab(event) {
        return Some(PeerScope::new(ReadOutput, PeerResource::Tab(tab)));
    }
    match event {
        SystemEvent::MuxStateChanged => Some(PeerScope::new(ReadMux, PeerResource::Host)),
        SystemEvent::ConfigChanged { .. } => Some(PeerScope::new(ReadHost, PeerResource::Host)),
        SystemEvent::Generic { event_type, .. } if event_type == "mux-state-changed" => {
            Some(PeerScope::new(ReadMux, PeerResource::Host))
        }
        SystemEvent::Generic { event_type, .. }
            if matches!(event_type.as_str(), "plan-changed" | "plans-loaded") =>
        {
            Some(PeerScope::new(ReadPlan, PeerResource::Host))
        }
        SystemEvent::Generic { event_type, .. }
            if matches!(
                event_type.as_str(),
                "config-changed"
                    | "appearance-committed"
                    | "provider-cli-started"
                    | "provider-cli-completed"
                    | "provider-cli-update-available"
            ) =>
        {
            Some(PeerScope::new(ReadHost, PeerResource::Host))
        }
        SystemEvent::Generic { event_type, data } if event_type == "tab-status-changed" => {
            let id = data
                .get("tab_id")
                .and_then(serde_json::Value::as_str)
                .filter(|id| !id.is_empty())?;
            Some(PeerScope::new(ReadTabs, PeerResource::Tab(id)))
        }
        _ => None,
    }
}

pub(super) fn peer_rail_frame(event: &SystemEvent) -> Option<(&'static str, serde_json::Value)> {
    if let Some(tab) = chat_send_status_tab(event) {
        return Some(("chat-send-status", serde_json::json!({ "tab_id": tab })));
    }
    match event {
        SystemEvent::ConfigChanged { .. } => Some(("config-changed", serde_json::json!({}))),
        SystemEvent::Generic { event_type, .. } if event_type == "config-changed" => {
            Some(("config-changed", serde_json::json!({})))
        }
        SystemEvent::Generic { event_type, data } if event_type == "appearance-committed" => {
            Some((
                "appearance-committed",
                serde_json::json!({
                    "event": data.get("event").and_then(serde_json::Value::as_str).unwrap_or(""),
                    "machine": data.get("machine").and_then(serde_json::Value::as_str).unwrap_or(""),
                    "proposal": data.get("proposal").and_then(serde_json::Value::as_str),
                    "undo": data.get("undo").and_then(serde_json::Value::as_bool).unwrap_or(false),
                }),
            ))
        }
        SystemEvent::Generic { event_type, data }
            if matches!(
                event_type.as_str(),
                "provider-cli-started" | "provider-cli-completed" | "provider-cli-update-available"
            ) =>
        {
            let name = match event_type.as_str() {
                "provider-cli-started" => "provider-cli-started",
                "provider-cli-completed" => "provider-cli-completed",
                _ => "provider-cli-update-available",
            };
            let mut payload = serde_json::json!({
                "provider": data.get("provider").and_then(serde_json::Value::as_str).unwrap_or(""),
            });
            if name == "provider-cli-completed"
                && data.get("operation").and_then(serde_json::Value::as_str) == Some("install")
            {
                if let Some(tab_id) = data.get("tab_id").and_then(serde_json::Value::as_str) {
                    payload["operation"] = serde_json::json!("install");
                    payload["tab_id"] = serde_json::json!(tab_id);
                }
            }
            if let Some(summary) = data.get("summary").and_then(serde_json::Value::as_str) {
                payload["summary"] = serde_json::json!(summary);
            }
            if let Some(success) = data.get("success").and_then(serde_json::Value::as_bool) {
                payload["success"] = serde_json::json!(success);
            }
            Some((name, payload))
        }
        _ => None,
    }
}

fn chat_send_status_tab(event: &SystemEvent) -> Option<&str> {
    let SystemEvent::Generic { event_type, data } = event else {
        return None;
    };
    if !matches!(
        event_type.as_str(),
        "chat-send-status" | "chat-send-queue-changed"
    ) {
        return None;
    }
    data.get("tab_id")
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.is_empty())
}

fn tab_topic<'a>(event: &SystemEvent, tab: &'a str) -> Option<PeerScope<'a>> {
    use PeerOperation::{ReadOutput, ReadPreview, ReadTabs};
    let SystemEvent::Generic { event_type, data } = event else {
        return None;
    };
    let operation = match event_type.as_str() {
        "pty-resize-owner-changed" | "provider-switch-status" | "tab-status-changed"
            if data.get("tab_id").and_then(serde_json::Value::as_str) == Some(tab) =>
        {
            ReadTabs
        }
        "agent-runtime-state"
            if data.get("tab_id").and_then(serde_json::Value::as_str) == Some(tab) =>
        {
            ReadOutput
        }
        name if name.strip_prefix("tab-chat-msg-") == Some(tab) => ReadOutput,
        name if name.strip_prefix("preview-index-state-") == Some(tab) => ReadPreview,
        _ => return None,
    };
    Some(PeerScope::new(operation, PeerResource::Tab(tab)))
}

#[cfg(test)]
mod tests;
