use super::EventsStreamQuery;
use crate::{auth::CallerIdentity, events::SystemEvent, routes::AppState};
use axum::http::{Method, StatusCode};
use codeterm_service_core::{api_support::access_policy::check_access, mesh::types::AccessLevel};
use serde::Deserialize;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PeerEventScope {
    Peer,
}

pub(super) fn validate_scope(
    query: &EventsStreamQuery,
    caller: Option<&CallerIdentity>,
) -> Result<(), (StatusCode, String)> {
    if query.scope.is_some() && query.tab_id.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Peer events do not take a tab filter".into(),
        ));
    }
    if query.scope.is_none()
        && matches!(
            caller,
            Some(CallerIdentity::Peer {
                access_level: AccessLevel::AgentIsolated,
                ..
            })
        )
    {
        return Err((
            StatusCode::FORBIDDEN,
            "Agent-isolated peers require scoped peer events".into(),
        ));
    }
    Ok(())
}

pub(super) fn visible(
    state: &AppState,
    caller: Option<&CallerIdentity>,
    scope: Option<PeerEventScope>,
    event: &SystemEvent,
) -> bool {
    if let Some(CallerIdentity::Peer { instance_id, .. }) = caller {
        let access = state
            .mesh
            .lock()
            .paired_peers
            .get(instance_id)
            .map(|peer| peer.access_level.clone());
        let Some(access) = access else { return false };
        return if scope.is_some() {
            topic_allowed(&access, event, |id| {
                state
                    .svc
                    .mux
                    .read()
                    .get_tab(id)
                    .is_some_and(|tab| crate::auth::peer_owns_tab(instance_id, tab))
            })
        } else {
            access != AccessLevel::AgentIsolated
                && check_access(&access, &Method::GET, "/api/events/stream")
        };
    }
    scope.is_none() || topic_allowed(&AccessLevel::Full, event, |_| false)
}

fn topic_allowed(
    access: &AccessLevel,
    event: &SystemEvent,
    owns_tab: impl FnOnce(&str) -> bool,
) -> bool {
    match event {
        SystemEvent::MuxStateChanged => check_access(access, &Method::GET, "/api/mux/state"),
        SystemEvent::Generic { event_type, data } => match event_type.as_str() {
            "mux-state-changed" => check_access(access, &Method::GET, "/api/mux/state"),
            "tab-status-changed" => data
                .get("tab_id")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|id| {
                    check_access(access, &Method::GET, "/api/tabs")
                        && (*access != AccessLevel::AgentIsolated || owns_tab(id))
                }),
            _ => false,
        },
        _ => false,
    }
}

#[cfg(test)]
mod tests;
