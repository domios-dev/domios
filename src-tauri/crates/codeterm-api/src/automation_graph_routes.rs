use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{
    auth::{caller_access_level, caller_peer_id},
    routes::AppState,
};
use codeterm_sentinel_host::graph::Snapshot;
use codeterm_sentinel_host::service_adapter::graph::explicit_team;
use codeterm_sentinel_host::AutomationGraphExt;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphQuery {
    workspace_id: String,
    team_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceGraphQuery {
    workspace_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopBody {
    workspace_id: String,
    team_id: Value,
    caller_tab_id: Option<String>,
    session_binding_nonce: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteBody {
    workspace_id: String,
    team_id: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventBody {
    plugin_id: String,
    event_id: String,
    payload: Value,
}

pub async fn event_ingress(
    State(state): State<AppState>,
    Path(subscription_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<EventBody>,
) -> Result<(StatusCode, Json<Value>), GraphError> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| {
            failure((
                StatusCode::UNAUTHORIZED,
                "subscription bearer required".into(),
            ))
        })?;
    let payload_bytes = serde_json::to_vec(&body.payload)
        .map_err(|error| failure((StatusCode::BAD_REQUEST, error.to_string())))?
        .len();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default();
    let decision = state
        .svc
        .ext::<codeterm_plugin::graph_events::SubscriptionTable>()
        .admit(codeterm_plugin::graph_events::AdmissionRequest {
            subscription_id: &subscription_id,
            token,
            plugin_id: &body.plugin_id,
            event_id: &body.event_id,
            payload: &body.payload,
            payload_bytes,
            now_ms,
        });
    match decision {
        codeterm_plugin::graph_events::AdmissionDecision::Admit => {
            if let Some(pipeline_id) = state
                .svc
                .ext::<codeterm_plugin::graph_events::SubscriptionTable>()
                .pipeline_id_for(&subscription_id)
            {
                if let Some(handle) = state
                    .svc
                    .host_extension::<codeterm_sentinel_host::scheduler::Handle>()
                {
                    handle.notify_event_subscription_changed(&pipeline_id);
                }
            }
            Ok((
                StatusCode::ACCEPTED,
                Json(json!({"ok":true,"admitted":true})),
            ))
        }
        codeterm_plugin::graph_events::AdmissionDecision::Refuse(reason) => {
            let (status, code) = admission_refusal_status(reason);
            if let Some(pipeline_id) = state
                .svc
                .ext::<codeterm_plugin::graph_events::SubscriptionTable>()
                .pipeline_id_for(&subscription_id)
            {
                if let Some(handle) = state
                    .svc
                    .host_extension::<codeterm_sentinel_host::scheduler::Handle>()
                {
                    handle.notify_event_subscription_changed(&pipeline_id);
                }
            }
            Err((status, Json(json!({"ok":false,"error":{"code":code}}))))
        }
    }
}

pub async fn snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<GraphQuery>,
) -> Result<Json<Snapshot>, GraphError> {
    caller_access_level(&state, &headers).map_err(failure)?;
    let team = query.team_id.ok_or_else(|| {
        failure((
            StatusCode::BAD_REQUEST,
            "team_id is required; empty means unteamed".into(),
        ))
    })?;
    let team = if team.is_empty() { None } else { Some(team) };
    let peer = caller_peer_id(&state, &headers).is_some();
    let mut snapshot = tokio::task::spawn_blocking(move || {
        state
            .svc
            .automation_graph_snapshot(&query.workspace_id, team.as_deref())
    })
    .await
    .map_err(|error| failure((StatusCode::INTERNAL_SERVER_ERROR, error.to_string())))?
    .map_err(|error| failure((StatusCode::BAD_REQUEST, error)))?;
    if peer {
        for node in &mut snapshot.nodes {
            if node.stop.allowed {
                node.stop.allowed = false;
                node.stop.reason = Some("peer-origin automation stop is unavailable".into());
            }
        }
    }
    Ok(Json(snapshot))
}

pub async fn workspace_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<WorkspaceGraphQuery>,
) -> Result<Json<Snapshot>, GraphError> {
    caller_access_level(&state, &headers).map_err(failure)?;
    let snapshot = tokio::task::spawn_blocking(move || {
        state
            .svc
            .automation_graph_workspace_snapshot(&query.workspace_id)
    })
    .await
    .map_err(|error| failure((StatusCode::INTERNAL_SERVER_ERROR, error.to_string())))?
    .map_err(|error| failure((StatusCode::BAD_REQUEST, error)))?;
    Ok(Json(snapshot))
}

pub async fn stop(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<StopBody>,
) -> Result<Json<Value>, GraphError> {
    caller_access_level(&state, &headers).map_err(failure)?;
    if caller_peer_id(&state, &headers).is_some() {
        return Err(failure((
            StatusCode::FORBIDDEN,
            "peer-origin automation stop is unavailable".into(),
        )));
    }
    let caller = crate::auth::caller_claim(&state, &headers, body.caller_tab_id.as_deref())
        .map_err(failure)?;
    let team = explicit_team(&body.team_id).map_err(|e| failure((StatusCode::BAD_REQUEST, e)))?;
    let result = tokio::task::spawn_blocking(move || {
        if let Some(tab) = caller {
            state.svc.automation_graph_stop_tab(
                &body.workspace_id,
                team.as_deref(),
                &id,
                &tab,
                body.session_binding_nonce.as_deref(),
            )
        } else {
            if body.session_binding_nonce.is_some() {
                return Err("session nonce requires caller_tab_id".into());
            }
            state
                .svc
                .automation_graph_stop_host(&body.workspace_id, team.as_deref(), &id)
        }
    })
    .await
    .map_err(|e| failure((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())))?
    .map_err(|e| failure((StatusCode::BAD_REQUEST, e)))?;
    Ok(Json(
        json!({"id":result.id,"state":"stopped","reason":result.last_reason}),
    ))
}

pub async fn delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<DeleteBody>,
) -> Result<StatusCode, GraphError> {
    caller_access_level(&state, &headers).map_err(failure)?;
    if caller_peer_id(&state, &headers).is_some() {
        return Err(failure((
            StatusCode::FORBIDDEN,
            "peer-origin automation delete is unavailable".into(),
        )));
    }
    let team = explicit_team(&body.team_id).map_err(|e| failure((StatusCode::BAD_REQUEST, e)))?;
    tokio::task::spawn_blocking(move || {
        state
            .svc
            .automation_graph_delete_host(&body.workspace_id, team.as_deref(), &id)
    })
    .await
    .map_err(|e| failure((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())))?
    .map_err(|e| failure((StatusCode::BAD_REQUEST, e)))?;
    Ok(StatusCode::NO_CONTENT)
}

type GraphError = (StatusCode, Json<Value>);

fn admission_refusal_status(
    reason: codeterm_plugin::graph_events::AdmitRefusal,
) -> (StatusCode, &'static str) {
    use codeterm_plugin::graph_events::AdmitRefusal;
    match reason {
        AdmitRefusal::UnknownSubscription => (StatusCode::NOT_FOUND, "unknown_subscription"),
        AdmitRefusal::BadToken => (StatusCode::UNAUTHORIZED, "bad_token"),
        AdmitRefusal::ForeignPlugin => (StatusCode::FORBIDDEN, "foreign_plugin"),
        AdmitRefusal::InvalidPayload => (StatusCode::UNPROCESSABLE_ENTITY, "invalid_payload"),
        AdmitRefusal::Oversize => (StatusCode::PAYLOAD_TOO_LARGE, "oversize"),
        AdmitRefusal::Duplicate => (StatusCode::CONFLICT, "duplicate"),
        AdmitRefusal::Backpressure => (StatusCode::SERVICE_UNAVAILABLE, "backpressure"),
        AdmitRefusal::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
    }
}

fn failure((status, message): (StatusCode, String)) -> GraphError {
    let (status, code) = if message.starts_with("blueprint_store_unsupported_format:") {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "blueprint_store_unsupported_format",
        )
    } else if message.starts_with("blueprint_store_load_failed:") {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "blueprint_store_load_failed",
        )
    } else if message.starts_with("blueprint_store_persistence_failed:") {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "blueprint_store_persistence_failed",
        )
    } else if status == StatusCode::UNAUTHORIZED {
        (status, "unauthorized")
    } else {
        (status, "automation_refused")
    };
    (
        status,
        Json(
            json!({"ok":false,"error":{"code":code,"message":message.replace("Sentinels", "Blueprints").replace("Sentinel", "Blueprint").replace("sentinel ", "blueprint ")}}),
        ),
    )
}

#[cfg(test)]
mod tests;
