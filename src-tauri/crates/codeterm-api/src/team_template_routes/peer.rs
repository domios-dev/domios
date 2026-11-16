//! Origin side of peer hosting: start a template on a paired peer Main, read an ask's outcome,
//! and transfer a template into a peer's My templates.

use axum::{extract::State, http::StatusCode, Json};
use codeterm_service_core::mesh::transport::PeerClient;
use codeterm_service_core::service::messenger::{MessageEnvelope, Messenger};
use codeterm_service_core::service::team_template_remote::hosting_gate;
use codeterm_service_core::service::ServiceState;
use codeterm_team_package::contracts::{
    TemplatePeerOutcomeState, TemplatePeerStartAccepted, TemplatePeerStartOutcome,
    TemplatePeerStartRequest, TemplatePeerStartState, TemplateStartResult, TemplateTransferRequest,
    TEMPLATE_PEER_OUTCOME_EVENT,
};
use codeterm_team_package::PackageFormat;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::routes::AppState;

type PeerRouteResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

/// Bounds how long the origin watches for the owner's answer; the ask itself stays open on the peer.
const ANSWER_WATCH_BOUND: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
const OUTCOME_CONTEXT: &str = "peer-template-start";

fn failure(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": message.into() })))
}

struct Peer {
    id: String,
    name: String,
    client: PeerClient,
}

/// A paired peer with Full access and a signed route, or why it cannot host.
fn select_peer(state: &AppState, target: &str) -> Result<Peer, (StatusCode, Json<Value>)> {
    let mesh = state.mesh.lock();
    let peer = codeterm_service_core::mesh::paired_resolve::resolve_paired_by_name(
        &mesh.paired_peers,
        target,
    )
    .map_err(|error| {
        failure(
            StatusCode::NOT_FOUND,
            format!("mesh peer resolution failed: {error:?}; use a paired instance ID"),
        )
    })?;
    if !crate::mesh_routes::peer_has_full_access(Some(&peer.access_level)) {
        return Err(failure(
            StatusCode::FORBIDDEN,
            "peer hosting requires Full mesh access",
        ));
    }
    let client = mesh
        .client_for(&peer.instance_id)
        .filter(|_| peer.online)
        .ok_or_else(|| {
            failure(
                StatusCode::SERVICE_UNAVAILABLE,
                format!(
                    "peer_offline: {} is offline; choose another placement",
                    peer.instance_name
                ),
            )
        })?;
    Ok(Peer {
        id: peer.instance_id.clone(),
        name: peer.instance_name.clone(),
        client,
    })
}

/// The peer's own identify decides; an unreachable identify is an offline peer.
async fn require_hosting(peer: &Peer) -> Result<(), (StatusCode, Json<Value>)> {
    let identify = peer
        .client
        .auth_get_json("/api/identify")
        .await
        .map_err(|error| {
            failure(
                StatusCode::SERVICE_UNAVAILABLE,
                format!(
                    "peer_offline: {} did not answer identify: {error}",
                    peer.name
                ),
            )
        })?;
    hosting_gate(&identify, &peer.name).map_err(|error| failure(StatusCode::CONFLICT, error))
}

/// A typed refusal the peer returned travels back typed.
fn peer_refusal(error: String) -> (StatusCode, Json<Value>) {
    let typed = error
        .find(codeterm_team_package::contracts::TEMPLATE_START_ERROR)
        .and_then(|start| TemplateStartResult::from_error_message(error[start..].trim()));
    match typed {
        Some(result) => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!(result))),
        None => failure(StatusCode::BAD_GATEWAY, error),
    }
}

/// Who asked for a peer start and where its outcome goes on this machine.
struct OutcomeRoute {
    request_id: String,
    peer_id: String,
    peer_name: String,
    origin_workspace_id: Option<String>,
    caller_tab_id: Option<String>,
}

/// Reads the peer's answer into the typed outcome; only a started Team names its managers.
pub(super) fn outcome_from(
    answer: &Value,
    request_id: &str,
    peer_id: &str,
    peer_name: &str,
    origin_workspace_id: Option<String>,
) -> TemplatePeerStartOutcome {
    let state = match answer.get("state").and_then(Value::as_str) {
        Some("started") => TemplatePeerOutcomeState::Started,
        Some("refused") => TemplatePeerOutcomeState::Refused,
        Some("failed") => TemplatePeerOutcomeState::Failed,
        Some("declined") => TemplatePeerOutcomeState::Declined,
        Some("expired") => TemplatePeerOutcomeState::Expired,
        Some("unanswered") => TemplatePeerOutcomeState::Unanswered,
        Some("stream_lost") => TemplatePeerOutcomeState::StreamLost,
        _ => TemplatePeerOutcomeState::Unknown,
    };
    let result = answer.get("result");
    let started = state == TemplatePeerOutcomeState::Started;
    let tab = |key: &str| {
        result
            .filter(|_| started)
            .and_then(|result| result.get(key))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    TemplatePeerStartOutcome {
        request_id: request_id.to_string(),
        peer_id: peer_id.to_string(),
        peer_name: peer_name.to_string(),
        state,
        origin_workspace_id,
        workspace_id: answer
            .get("workspace_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        orchestrator_tab_id: tab("orchestrator_tab_id"),
        planner_tab_id: tab("planner_tab_id"),
        refusal: result
            .filter(|_| state == TemplatePeerOutcomeState::Refused)
            .and_then(|result| serde_json::from_value(result.clone()).ok()),
        error: answer
            .get("error")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

/// Starts a template on a paired peer Main in the peer workspace the ask names. A peer that
/// starts at once answers with the outcome; otherwise its owner decides there and the origin
/// watches the peer's event stream for that answer. Every outcome is published here as
/// `team-template-peer-outcome`, so the origin can show the peer's managers.
pub(super) async fn peer_start(
    State(state): State<AppState>,
    Json(body): Json<TemplatePeerStartRequest>,
) -> PeerRouteResult {
    let peer = select_peer(&state, &body.peer)?;
    require_hosting(&peer).await?;
    // Subscribe before asking so the answer can never land between the two.
    let stream = peer
        .client
        .open_relay_stream("/api/events/stream")
        .await
        .map_err(|error| failure(StatusCode::SERVICE_UNAVAILABLE, error))?;
    if !stream.status().is_success() {
        return Err(failure(
            StatusCode::BAD_GATEWAY,
            format!("peer event stream refused: {}", stream.status()),
        ));
    }
    let asked = peer
        .client
        .auth_post_json("/api/mesh/template-start", &body.start)
        .await
        .map_err(peer_refusal)?;
    let Peer { id, name, client } = peer;
    let notification_id = asked
        .get("notification_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let request_id = asked
        .get("request_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("ps_{}", uuid::Uuid::new_v4().simple()));
    let route = OutcomeRoute {
        request_id: request_id.clone(),
        peer_id: id.clone(),
        peer_name: name.clone(),
        origin_workspace_id: body.origin_workspace_id,
        caller_tab_id: body.caller_tab_id,
    };
    let Some(notification_id) = notification_id else {
        drop(stream);
        let outcome = outcome_from(
            &asked,
            &route.request_id,
            &route.peer_id,
            &route.peer_name,
            route.origin_workspace_id.clone(),
        );
        publish_outcome(state.svc.clone(), outcome.clone(), route.caller_tab_id);
        return Ok(Json(json!(TemplatePeerStartAccepted {
            state: TemplatePeerStartState::Finished,
            peer_id: id,
            peer_name: name,
            request_id,
            notification_id: None,
            outcome: Some(outcome),
        })));
    };
    tokio::spawn(watch_answer(
        state.svc.clone(),
        client,
        stream,
        route,
        notification_id.clone(),
    ));
    Ok(Json(json!(TemplatePeerStartAccepted {
        state: TemplatePeerStartState::AwaitingOwner,
        peer_id: id,
        peer_name: name,
        request_id,
        notification_id: Some(notification_id),
        outcome: None,
    })))
}

/// One SSE line names the answer to this question.
pub(super) fn answered_frame(line: &[u8], notification_id: &str) -> bool {
    let Some(data) = line.strip_prefix(b"data:") else {
        return false;
    };
    let Ok(frame) = serde_json::from_slice::<Value>(data) else {
        return false;
    };
    frame.get("event_type").and_then(Value::as_str) == Some("notification-answered")
        && frame
            .get("data")
            .and_then(|data| data.get("id"))
            .and_then(Value::as_str)
            == Some(notification_id)
}

async fn answer_arrived(mut stream: reqwest::Response, notification_id: &str) -> bool {
    let mut buffer = Vec::new();
    while let Ok(Some(chunk)) = stream.chunk().await {
        buffer.extend_from_slice(&chunk);
        while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = buffer.drain(..=end).collect();
            if answered_frame(line.trim_ascii(), notification_id) {
                return true;
            }
        }
    }
    false
}

async fn watch_answer(
    svc: ServiceState,
    client: PeerClient,
    stream: reqwest::Response,
    route: OutcomeRoute,
    notification_id: String,
) {
    let request_id = route.request_id.clone();
    let answer =
        match tokio::time::timeout(ANSWER_WATCH_BOUND, answer_arrived(stream, &notification_id))
            .await
        {
            Ok(true) => client
                .auth_post_json(
                    "/api/mesh/peer-ask-outcome",
                    &json!({ "request_id": request_id }),
                )
                .await
                .unwrap_or_else(|error| json!({ "state": "unknown", "error": error })),
            Ok(false) => json!({ "state": "stream_lost" }),
            Err(_) => json!({ "state": "unanswered" }),
        };
    let outcome = outcome_from(
        &answer,
        &route.request_id,
        &route.peer_id,
        &route.peer_name,
        route.origin_workspace_id,
    );
    publish_outcome(svc, outcome, route.caller_tab_id);
}

/// Tells this machine what became of a peer start: the system notice, the typed event the
/// frontend attaches the peer's managers from, and the caller tab when one asked.
fn publish_outcome(
    svc: ServiceState,
    outcome: TemplatePeerStartOutcome,
    caller_tab_id: Option<String>,
) {
    let state = serde_json::to_value(outcome.state)
        .ok()
        .and_then(|state| state.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into());
    let peer_name = outcome.peer_name.clone();
    let request_id = outcome.request_id.clone();
    let detail = json!(outcome);
    let body = format!(
        "Template start on {peer_name} (request {request_id}): {state}.\n{detail}\nRead it again with `domios team template outcome {request_id} --mesh {peer_name}`."
    );
    tokio::task::spawn_blocking(move || {
        svc.notify_event(TEMPLATE_PEER_OUTCOME_EVENT, detail);
        svc.log_system_notice(
            format!("Template start on {peer_name}: {state}"),
            Some(body.clone()),
            Some(if outcome.state == TemplatePeerOutcomeState::Started {
                "info"
            } else {
                "warning"
            }),
        );
        if let Some(tab) = caller_tab_id.filter(|tab| !tab.trim().is_empty()) {
            if let Err(error) = Messenger::deliver(
                &svc,
                &tab,
                MessageEnvelope::codeterm_reminder(OUTCOME_CONTEXT),
                &body,
            ) {
                log::warn!("[peer-hosting] outcome delivery to {tab} failed: {error}");
            }
        }
    });
}

#[derive(Deserialize)]
pub(super) struct PeerOutcomeBody {
    peer: String,
    request_id: String,
}

/// Reads a peer ask's outcome once, e.g. after the origin restarted while it was open.
pub(super) async fn peer_outcome(
    State(state): State<AppState>,
    Json(body): Json<PeerOutcomeBody>,
) -> PeerRouteResult {
    let peer = select_peer(&state, &body.peer)?;
    peer.client
        .auth_post_json(
            "/api/mesh/peer-ask-outcome",
            &json!({ "request_id": body.request_id }),
        )
        .await
        .map(Json)
        .map_err(|error| failure(StatusCode::BAD_GATEWAY, error))
}

#[derive(Deserialize)]
pub(super) struct TransferBody {
    #[serde(flatten)]
    request: TemplateTransferRequest,
    /// The peer tab the attachment rail delivers the package to, chosen by the owner here.
    tab: String,
}

/// Exports through the one exporter as `.team`, carries it over the mesh attachment rail and
/// imports it on the peer, where it lands unverified in My templates.
pub(super) async fn transfer(
    State(state): State<AppState>,
    Json(body): Json<TransferBody>,
) -> PeerRouteResult {
    if body.request.format != PackageFormat::Team {
        return Err(failure(
            StatusCode::BAD_REQUEST,
            "templates travel to a peer as a `.team` archive",
        ));
    }
    let peer = select_peer(&state, &body.request.peer)?;
    let svc = state.svc.clone();
    let export = json!({ "template": body.request.template, "format": body.request.format });
    let exported = tokio::task::spawn_blocking(move || {
        codeterm_service_core::service::team_exchange::export(svc.as_ref(), Some(&export))
    })
    .await
    .map_err(|error| failure(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| failure(StatusCode::UNPROCESSABLE_ENTITY, error.message))?;
    let path = exported
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| failure(StatusCode::INTERNAL_SERVER_ERROR, "export wrote no package"))?
        .to_string();
    let ingested = crate::mesh_attachment::transfer_to_peer(
        &state.mesh,
        &format!("mesh:{}/{}", peer.id, body.tab),
        &peer.id,
        path,
        codeterm_service_core::service::mesh_attachment::AttachmentKind::File,
    )
    .await
    .map_err(|error| failure(StatusCode::BAD_GATEWAY, error))?;
    let imported = peer
        .client
        .send_relay(&codeterm_service_core::mesh::types::RelayCommand {
            method: "POST".into(),
            path: "/api/team-templates/import".into(),
            body: json!({ "path": ingested.path }),
            delivery_id: None,
        })
        .await
        .map_err(|error| failure(StatusCode::BAD_GATEWAY, error.to_string()))?;
    if !imported.ok {
        return Err(failure(
            StatusCode::from_u16(imported.status).unwrap_or(StatusCode::BAD_GATEWAY),
            imported.error.unwrap_or_else(|| {
                imported
                    .data
                    .map(|data| data.to_string())
                    .unwrap_or_default()
            }),
        ));
    }
    Ok(Json(json!({
        "peer_id": peer.id,
        "peer_name": peer.name,
        "path": ingested.path,
        "import": imported.data,
    })))
}

#[cfg(test)]
mod tests {
    use super::{answered_frame, outcome_from};
    use codeterm_team_package::contracts::TemplatePeerOutcomeState;
    use serde_json::json;

    #[test]
    fn only_a_started_answer_names_the_peer_managers() {
        let started = json!({
            "state": "started",
            "workspace_id": "ws-peer",
            "result": { "orchestrator_tab_id": "t-orch", "planner_tab_id": "t-plan" },
        });
        let outcome = outcome_from(&started, "r-1", "p-1", "peer", Some("ws-home".into()));
        assert_eq!(outcome.state, TemplatePeerOutcomeState::Started);
        assert_eq!(outcome.orchestrator_tab_id.as_deref(), Some("t-orch"));
        assert_eq!(outcome.planner_tab_id.as_deref(), Some("t-plan"));
        assert_eq!(outcome.workspace_id.as_deref(), Some("ws-peer"));
        assert_eq!(outcome.origin_workspace_id.as_deref(), Some("ws-home"));

        let failed = json!({
            "state": "failed",
            "error": "ERROR-FRAGMENT",
            "result": { "orchestrator_tab_id": "t-orch" },
        });
        let outcome = outcome_from(&failed, "r-2", "p-1", "peer", None);
        assert_eq!(outcome.state, TemplatePeerOutcomeState::Failed);
        assert_eq!(outcome.orchestrator_tab_id, None);
        assert_eq!(outcome.error.as_deref(), Some("ERROR-FRAGMENT"));

        let unknown = outcome_from(
            &json!({ "state": "awaiting_owner" }),
            "r-3",
            "p-1",
            "peer",
            None,
        );
        assert_eq!(unknown.state, TemplatePeerOutcomeState::Unknown);
    }

    #[test]
    fn only_the_answer_to_this_question_ends_the_watch() {
        let frame = br#"data: {"prev_seq":1,"boot":"b","data":{"id":"n-1","answered":"Start here","delivered":true},"event_type":"notification-answered","seq":2}"#;
        assert!(answered_frame(frame, "n-1"));
        assert!(!answered_frame(frame, "n-2"));
        let other = br#"data: {"boot":"b","data":{"id":"n-1"},"event_type":"notification-created","seq":3}"#;
        assert!(!answered_frame(other, "n-1"));
        assert!(!answered_frame(b": keepalive", "n-1"));
    }
}
