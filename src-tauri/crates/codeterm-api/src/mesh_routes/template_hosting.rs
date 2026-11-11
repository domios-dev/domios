//! Peer Main hosting over mesh: the keepawake port behind hosted-role leases and the
//! mesh-origin template start, which runs this machine's ordinary pair rail.

use std::sync::Arc;

use axum::{extract::State as AxumState, http::StatusCode, Json};
use codeterm_orchestration::OrchestrationPairExt;
use codeterm_service_core::ports::host::UiEvents;
use codeterm_service_core::service::team_template_remote::{
    admit_from_peer, preview_for_mesh_start, unbound_secrets, AwakePort, HostingLeases,
    PeerStartRefusal, SpentDecisions,
};
use codeterm_service_core::service::team_template_start::{ConsentLedger, TemplateCallError};
use codeterm_team_package::contracts::{TemplateStartRequest, TemplateStartResult};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::routes::AppState;

/// Bounds a keepawake whose holders vanished without a release, such as across a restart.
const LEASE_BOUND_SECS: u64 = 12 * 60 * 60;

struct KeepAwakeLeasePort {
    keepawake: codeterm_service_core::KeepAwakeStateHandle,
    ui: Arc<dyn UiEvents>,
}

impl AwakePort for KeepAwakeLeasePort {
    fn hold(&self) -> Result<bool, String> {
        if codeterm_service_core::keepawake::status(&self.keepawake).active {
            return Ok(false);
        }
        codeterm_service_core::keepawake::start(
            &self.keepawake,
            &self.ui,
            Some(LEASE_BOUND_SECS),
            Some(false),
            None,
            None,
        )
        .map(|_| true)
    }

    fn release(&self) -> Result<(), String> {
        codeterm_service_core::keepawake::stop(&self.keepawake, &self.ui)
    }
}

/// Holds this host awake for a hosted role; a keepawake failure is reported, never fatal.
pub(super) fn acquire_hosting_lease(state: &AppState, holder: &str) -> String {
    let port = Arc::new(KeepAwakeLeasePort {
        keepawake: state.keepawake.clone(),
        ui: state.svc.ui.clone(),
    });
    match HostingLeases::global().acquire(holder, port) {
        Ok(()) => "held".into(),
        Err(error) => {
            log::warn!("[peer-hosting] keepawake for {holder} unavailable: {error}");
            format!("unavailable: {error}")
        }
    }
}

#[derive(Deserialize)]
pub struct MeshTemplateStartRequest {
    template: TemplateStartRequest,
    #[serde(default)]
    orchestrator_agent: String,
    #[serde(default)]
    orchestrator_model: Option<String>,
    #[serde(default)]
    planner_agent: String,
    #[serde(default)]
    planner_model: Option<String>,
}

fn refusal_response(refusal: PeerStartRefusal) -> (StatusCode, String) {
    let message = refusal.message();
    match refusal {
        PeerStartRefusal::Forbidden => (StatusCode::FORBIDDEN, message),
        PeerStartRefusal::OriginTeam | PeerStartRefusal::DecisionSpent => {
            (StatusCode::CONFLICT, message)
        }
        PeerStartRefusal::Refused(refusal) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            TemplateStartResult::Refused { refusal }.error_message(),
        ),
    }
}

/// A paired peer asks this Main to start a checked package it already holds. The owner's
/// decision was issued here; secrets bind from this machine's vault by name; the Team is new.
pub async fn mesh_template_start(
    AxumState(state): AxumState<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<MeshTemplateStartRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let origin = super::relay::check_mesh_relay_auth(&state, &headers)?;
    let full_access = state
        .mesh
        .lock()
        .paired_peers
        .get(&origin)
        .is_some_and(|peer| super::peer_has_full_access(Some(&peer.access_level)));
    let mut present = Vec::new();
    {
        let vault = state.svc.secrets.read().await;
        for entry in request.template.secret_bindings.values() {
            let entry = entry.trim();
            if !entry.is_empty() {
                present.push((entry.to_string(), vault.has(entry).await.unwrap_or(false)));
            }
        }
    }
    let unbound = unbound_secrets(&request.template, |entry| {
        Ok(present.iter().any(|(name, found)| name == entry && *found))
    });
    let template = request.template.clone();
    tokio::task::spawn_blocking(move || {
        admit_from_peer(
            &template,
            full_access,
            ConsentLedger::global(),
            SpentDecisions::global(),
            unbound,
        )
        .map(|_| ())
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(refusal_response)?;
    let pair = codeterm_orchestration::OrchestrationPairParams {
        orchestrator_agent: request.orchestrator_agent,
        orchestrator_model: request.orchestrator_model,
        planner_agent: request.planner_agent,
        planner_model: request.planner_model,
        template: Some(request.template),
        ..Default::default()
    };
    let result = state
        .svc
        .create_orchestration_pair(pair)
        .await
        .map_err(|error| (StatusCode::CONFLICT, error))?;
    serde_json::to_value(result)
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

/// The owner previews on this machine a start a peer will request; the decision is
/// always issued because a mesh start always needs one.
pub async fn preview_mesh_start(
    AxumState(state): AxumState<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let svc = state.svc.clone();
    let outcome = tokio::task::spawn_blocking(move || preview_for_mesh_start(&svc, Some(&body)))
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
        })?;
    match outcome {
        Ok(preview) => serde_json::to_value(preview).map(Json).map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
        }),
        Err(TemplateCallError::InvalidParams(message)) => {
            Err((StatusCode::BAD_REQUEST, Json(json!({ "error": message }))))
        }
        Err(TemplateCallError::Refused(refusal)) => Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!(TemplateStartResult::Refused { refusal })),
        )),
    }
}
