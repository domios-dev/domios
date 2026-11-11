//! Peer Main hosting over mesh: a paired origin asks this machine to start a template or to
//! clone a missing project. Each ask becomes an ordinary action_needed daemon question to
//! this machine's owner; only the owner's answer runs it, on this peer's own rails.

use axum::{extract::State as AxumState, http::StatusCode, Json};
use codeterm_orchestration::OrchestrationPairExt;
use codeterm_service_core::ports::daemon_question::DaemonQuestionHandler;
use codeterm_service_core::service::team_template_remote::{
    admit_from_peer, clone_destination, precheck_peer_start, record_owner_decision,
    scrub_remote_url, unbound_secrets, PeerAsk, PeerAsks, PeerStartAsk, PeerStartRefusal,
    SpentDecisions, CLONE_ACCEPT, PEER_ASK_DECLINE, PEER_ASK_PREFIX, START_ACCEPT,
};
use codeterm_service_core::service::team_template_start::ConsentLedger;
use codeterm_service_core::service::ServiceState;
use codeterm_team_package::contracts::{TemplateStartRequest, TemplateStartResult};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::routes::AppState;

fn refusal_response(refusal: PeerStartRefusal) -> (StatusCode, String) {
    let message = refusal.message();
    match refusal {
        PeerStartRefusal::Forbidden => (StatusCode::FORBIDDEN, message),
        PeerStartRefusal::OriginTeam | PeerStartRefusal::DecisionSpent => {
            (StatusCode::CONFLICT, message)
        }
        PeerStartRefusal::OwnerUnreachable(_) => (StatusCode::SERVICE_UNAVAILABLE, message),
        PeerStartRefusal::Refused(refusal) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            TemplateStartResult::Refused { refusal }.error_message(),
        ),
    }
}

fn refusal_outcome(refusal: PeerStartRefusal) -> Value {
    match refusal {
        PeerStartRefusal::Refused(refusal) => {
            json!({ "state": "refused", "result": TemplateStartResult::Refused { refusal } })
        }
        other => json!({ "state": "refused", "error": other.message() }),
    }
}

fn origin_access(state: &AppState, origin: &str) -> (bool, String) {
    let mesh = state.mesh.lock();
    let peer = mesh.paired_peers.get(origin);
    (
        peer.is_some_and(|peer| super::peer_has_full_access(Some(&peer.access_level))),
        peer.map_or_else(|| origin.to_string(), |peer| peer.instance_name.clone()),
    )
}

fn service_full_access(svc: &ServiceState, origin: &str) -> bool {
    svc.ext::<codeterm_service_core::MeshStateHandle>()
        .lock()
        .paired_peers
        .get(origin)
        .is_some_and(|peer| super::peer_has_full_access(Some(&peer.access_level)))
}

/// Presence of each named entry in this machine's vault; values are never read.
async fn vault_presence(state: &AppState, request: &TemplateStartRequest) -> Vec<(String, bool)> {
    let vault = state.svc.secrets.read().await;
    let mut present = Vec::new();
    for entry in request.secret_bindings.values() {
        let entry = entry.trim();
        if !entry.is_empty() {
            present.push((entry.to_string(), vault.has(entry).await.unwrap_or(false)));
        }
    }
    present
}

fn unbound_from(request: &TemplateStartRequest, present: &[(String, bool)]) -> Vec<String> {
    unbound_secrets(request, |entry| {
        Ok(present.iter().any(|(name, found)| name == entry && *found))
    })
}

/// A paired origin asks this Main to start a checked package it holds. Nothing starts here
/// until this machine's owner accepts the question; the answer returns over the event stream.
pub async fn mesh_template_start(
    AxumState(state): AxumState<AppState>,
    headers: axum::http::HeaderMap,
    Json(ask): Json<PeerStartAsk>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let origin = super::relay::check_mesh_relay_auth(&state, &headers)?;
    let (full_access, origin_name) = origin_access(&state, &origin);
    let present = vault_presence(&state, &ask.template).await;
    let unbound = unbound_from(&ask.template, &present);
    let svc = state.svc.clone();
    let opened = tokio::task::spawn_blocking(move || {
        precheck_peer_start(&ask.template, full_access, unbound)?;
        let template = &ask.template.template;
        let name = match &template.version {
            Some(version) => format!("{}@{version}", template.name),
            None => template.name.clone(),
        };
        let request = PeerAsks::global().open(PeerAsk::Start {
            origin_peer: origin,
            ask,
        });
        let question = svc.publish_daemon_question(
            request.clone(),
            format!("Start team template {name} here?"),
            format!(
                "{origin_name} asks this machine to start `{name}` as a new Team; its managers \
                 will run here. Secrets bind from this machine's vault by name. Nothing of the \
                 origin's Team, plans or secret values is copied."
            ),
            vec![START_ACCEPT.into(), PEER_ASK_DECLINE.into()],
        );
        match question {
            Ok(notification_id) => Ok((request, notification_id)),
            Err(error) => {
                PeerAsks::global().take(&request);
                Err(PeerStartRefusal::OwnerUnreachable(error))
            }
        }
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let (request, notification_id) = opened.map_err(refusal_response)?;
    Ok(Json(json!({
        "state": "awaiting_owner",
        "request_id": request,
        "notification_id": notification_id,
    })))
}

#[derive(Deserialize)]
pub struct PeerAskOutcomeRequest {
    request_id: String,
}

/// The finished outcome of an ask this origin opened, read once its answer event arrived.
pub async fn mesh_peer_ask_outcome(
    AxumState(state): AxumState<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<PeerAskOutcomeRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let origin = super::relay::check_mesh_relay_auth(&state, &headers)?;
    if !origin_access(&state, &origin).0 {
        return Err((
            StatusCode::FORBIDDEN,
            "peer ask outcomes require Full mesh access".into(),
        ));
    }
    PeerAsks::global()
        .outcome(&request.request_id)
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "unknown peer ask".into()))
}

/// A missing project with a clone URL from the origin: the owner of this peer is asked once
/// whether to clone it. Returns what the refusal tells the origin.
pub(super) fn ask_clone(state: &AppState, origin: &str, url: &str) -> String {
    if scrub_remote_url(url).as_deref() != Some(url) {
        return "a clone URL carrying credentials or no usable remote was refused".into();
    }
    if let Some(request) = PeerAsks::global().open_clone(url) {
        return format!("the owner of this peer was already asked to clone {url} (request {request}); dispatch again after they accept");
    }
    let Some(path) = dirs::home_dir()
        .map(|home| home.join("Developer"))
        .and_then(|root| clone_destination(&root, url))
    else {
        return format!("no clone destination fits {url}");
    };
    if path.exists() {
        return format!(
            "{} already exists without a matching checkout; open it as a workspace here or choose another placement",
            path.display()
        );
    }
    let origin_name = origin_access(state, origin).1;
    let request = PeerAsks::global().open(PeerAsk::Clone {
        origin_peer: origin.to_string(),
        url: url.to_string(),
        path: path.clone(),
    });
    match state.svc.publish_daemon_question(
        request.clone(),
        "Clone a project for a peer?".into(),
        format!(
            "{origin_name} dispatched a Worker for a project this machine lacks. Clone {url} into {}?",
            path.display()
        ),
        vec![CLONE_ACCEPT.into(), PEER_ASK_DECLINE.into()],
    ) {
        Ok(_) => format!(
            "asked the owner of this peer to clone {url} into {} (request {request}); dispatch again after they accept",
            path.display()
        ),
        Err(error) => {
            PeerAsks::global().take(&request);
            format!("could not ask the owner of this peer to clone: {error}")
        }
    }
}

/// Answers the owner's tap on a peer-hosting question; the work runs here, once.
pub struct PeerHostingQuestions {
    svc: ServiceState,
}

impl PeerHostingQuestions {
    pub fn new(svc: ServiceState) -> Self {
        Self { svc }
    }
}

impl DaemonQuestionHandler for PeerHostingQuestions {
    fn answer(&self, request: &str, label: &str) -> Option<Result<Value, String>> {
        if !request.starts_with(PEER_ASK_PREFIX) {
            return None;
        }
        let svc = self.svc.clone();
        let request = request.to_string();
        let label = label.to_string();
        // The pair and the vault are async; a fresh thread keeps block_on off any runtime worker.
        let outcome = std::thread::spawn(move || answer_ask(&svc, &request, &label))
            .join()
            .unwrap_or_else(|_| json!({ "state": "failed", "error": "peer ask panicked" }));
        Some(Ok(outcome))
    }
}

fn answer_ask(svc: &ServiceState, request: &str, label: &str) -> Value {
    let Some(ask) = PeerAsks::global().take(request) else {
        return json!({ "state": "expired" });
    };
    let outcome = match ask {
        PeerAsk::Start { origin_peer, ask } if label == START_ACCEPT => {
            start_accepted(svc, &origin_peer, ask)
        }
        PeerAsk::Clone { url, path, .. } if label == CLONE_ACCEPT => {
            clone_accepted(svc, &url, &path)
        }
        _ => json!({ "state": "declined" }),
    };
    PeerAsks::global().finish(request, outcome.clone());
    outcome
}

fn start_accepted(svc: &ServiceState, origin: &str, ask: PeerStartAsk) -> Value {
    let present = codeterm_service_core::async_runtime::block_on(async {
        let vault = svc.secrets.read().await;
        let mut present = Vec::new();
        for entry in ask.template.secret_bindings.values() {
            let entry = entry.trim();
            if !entry.is_empty() {
                present.push((entry.to_string(), vault.has(entry).await.unwrap_or(false)));
            }
        }
        present
    });
    let unbound = unbound_from(&ask.template, &present);
    let decided = match record_owner_decision(&ask.template, ConsentLedger::global()) {
        Ok(decided) => decided,
        Err(refusal) => return refusal_outcome(PeerStartRefusal::Refused(refusal)),
    };
    if let Err(refusal) = admit_from_peer(
        &decided,
        service_full_access(svc, origin),
        ConsentLedger::global(),
        SpentDecisions::global(),
        unbound,
    ) {
        return refusal_outcome(refusal);
    }
    let pair = codeterm_orchestration::OrchestrationPairParams {
        orchestrator_agent: ask.orchestrator_agent,
        orchestrator_model: ask.orchestrator_model,
        planner_agent: ask.planner_agent,
        planner_model: ask.planner_model,
        template: Some(decided),
        ..Default::default()
    };
    match codeterm_service_core::async_runtime::block_on(svc.create_orchestration_pair(pair)) {
        Ok(result) => json!({ "state": "started", "result": result }),
        Err(message) => match TemplateStartResult::from_error_message(&message) {
            Some(result) => json!({ "state": "refused", "result": result }),
            None => json!({ "state": "failed", "error": message }),
        },
    }
}

fn clone_accepted(svc: &ServiceState, url: &str, path: &std::path::Path) -> Value {
    if path.exists() {
        return json!({ "state": "failed", "error": format!("{} already exists", path.display()) });
    }
    if let Some(parent) = path.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            return json!({ "state": "failed", "error": error.to_string() });
        }
    }
    let mut command = std::process::Command::new("git");
    codeterm_service_core::process_util::no_window_std(&mut command);
    let output = command
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["clone", "--"])
        .arg(url)
        .arg(path)
        .output();
    match output {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            return json!({
                "state": "failed",
                "error": codeterm_util::redact::text(String::from_utf8_lossy(&output.stderr).trim()),
            })
        }
        Err(error) => return json!({ "state": "failed", "error": error.to_string() }),
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".into());
    match svc.create_workspace(name, Some(path.display().to_string()), false) {
        Ok(workspace) => json!({
            "state": "cloned",
            "path": path.display().to_string(),
            "workspace_id": workspace.id,
        }),
        Err(error) => json!({
            "state": "failed",
            "path": path.display().to_string(),
            "error": format!("cloned but no workspace was opened: {error}"),
        }),
    }
}
