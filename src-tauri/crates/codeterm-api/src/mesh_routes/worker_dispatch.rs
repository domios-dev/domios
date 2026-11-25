use axum::{extract::State as AxumState, http::StatusCode, Json};
use serde::Deserialize;
use serde::Serialize;

use crate::routes::AppState;

const MAX_LANE_BUNDLE_BYTES: u64 = 32 * 1024 * 1024;
const REMOTE_LAUNCH_READY_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

fn lane_bundle_size_allowed(size: u64, limit: u64) -> bool {
    size <= limit
}

fn provider_auth_required_message(
    provider_id: &str,
    display_name: &str,
    peer_name: &str,
) -> String {
    format!(
        "peer_provider_auth_required: {display_name} ('{provider_id}') is not signed in on peer '{peer_name}'; sign in on {peer_name} before dispatching"
    )
}

fn provider_display_name(
    svc: &codeterm_service_core::service::ServiceContext,
    provider_id: &str,
) -> String {
    svc.agent_registry
        .with_provider(provider_id, |provider| provider.display_name().to_string())
        .unwrap_or_else(|| provider_id.to_string())
}

fn cleanup_suffix(cleanup: Result<(), String>) -> String {
    match cleanup {
        Ok(()) => String::new(),
        Err(error) => format!("; lane cleanup failed: {error}"),
    }
}

fn classify_spawn_failure(
    svc: &codeterm_service_core::service::ServiceContext,
    provider_id: &str,
    peer_name: &str,
    error: &str,
    cleanup: Result<(), String>,
) -> (StatusCode, String) {
    let suffix = cleanup_suffix(cleanup);
    if codeterm_service_core::service::agent_runtime::transport::is_sign_in_required_launch_error(
        error,
    ) {
        let display_name = provider_display_name(svc, provider_id);
        return (
            StatusCode::CONFLICT,
            format!(
                "{}{suffix}",
                provider_auth_required_message(provider_id, &display_name, peer_name)
            ),
        );
    }
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("peer_worker_spawn_failed: {error}{suffix}"),
    )
}

fn preflight_remote_worker_provider(
    svc: &codeterm_service_core::service::ServiceContext,
    provider_id: &str,
    workspace_id: &str,
    peer_name: &str,
) -> Result<(), (StatusCode, String)> {
    if svc.agent_registry.provider_arc(provider_id).is_none()
        && svc.chatbackend_registry.get(provider_id).is_some()
    {
        return Ok(());
    }
    codeterm_service_core::service::spawn::provider_launch_preflight(
        &svc.agent_registry,
        provider_id,
        true,
    )
    .map_err(|error| {
        (
            StatusCode::CONFLICT,
            format!("peer_worker_spawn_failed: {error}"),
        )
    })?;
    match codeterm_service_core::service::spawn_cli::provider_auth_preflight(
        svc,
        provider_id,
        Some(workspace_id),
    ) {
        Ok(
            codeterm_service_core::service::spawn_cli::ProviderAuthPreflightOutcome::Inconclusive(
                reason,
            ),
        ) => {
            log::warn!(
                "[remote-dispatch] auth preflight inconclusive for provider={} on peer={}: {}",
                provider_id,
                peer_name,
                reason
            );
            Ok(())
        }
        Ok(_) => Ok(()),
        Err(required) => Err((
            StatusCode::CONFLICT,
            provider_auth_required_message(
                &required.provider_id,
                &required.display_name,
                peer_name,
            ),
        )),
    }
}

fn observe_failed_launch_state(
    svc: &codeterm_service_core::service::ServiceContext,
    tab_id: &str,
) -> Option<String> {
    let mux = svc.mux.read();
    let Some(tab) = mux.get_tab(tab_id) else {
        return Some("worker tab disappeared during launch".into());
    };
    if tab
        .agent_runtime_state
        .as_ref()
        .and_then(|state| state.get("auth"))
        .and_then(serde_json::Value::as_str)
        == Some("signedOut")
    {
        return Some(
            codeterm_service_core::service::agent_runtime::model::RuntimeFailure::SignInRequired
                .to_string(),
        );
    }
    tab.boot_failure_reason.clone()
}

fn await_remote_launch_outcome(
    svc: &codeterm_service_core::service::ServiceContext,
    tab_id: &str,
    ready_rx: tokio::sync::oneshot::Receiver<Result<(), String>>,
    timeout: std::time::Duration,
) -> Result<(), String> {
    match codeterm_service_core::async_runtime::block_on(tokio::time::timeout(timeout, ready_rx)) {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(error))) => Err(error),
        Ok(Err(_)) | Err(_) => match observe_failed_launch_state(svc, tab_id) {
            Some(error) => Err(error),
            None => Ok(()),
        },
    }
}

#[derive(Clone, Deserialize)]
pub struct MeshWorkerDispatchRequest {
    repository_identity: String,
    #[serde(default)]
    repository_identities: Vec<String>,
    base_commit: String,
    plan_id: String,
    task_id: String,
    attempt_id: String,
    origin_tab_id: String,
    provider: String,
    model: Option<String>,
    reasoning_effort: Option<String>,
    speed: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    task_description_managed: bool,
    brief: String,
    /// The origin's credential-free remote URL; a missing project is cloned only on owner consent.
    #[serde(default)]
    clone_url: Option<String>,
}

#[derive(Deserialize)]
pub struct MeshWorkerLaneBundleRequest {
    repository_identity: String,
    #[serde(default)]
    repository_identities: Vec<String>,
    branch: String,
    base_commit: String,
    plan_id: String,
    task_id: String,
    attempt_id: String,
    report_sha: String,
}

pub async fn mesh_worker_lane_bundle(
    AxumState(state): AxumState<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<MeshWorkerLaneBundleRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let peer_id = super::relay::check_mesh_relay_auth(&state, &headers)?;
    tokio::task::spawn_blocking(move || create_lane_bundle(state, peer_id, request))
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

fn create_lane_bundle(
    state: AppState,
    peer_id: String,
    request: MeshWorkerLaneBundleRequest,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    create_lane_bundle_with_limit(state, peer_id, request, MAX_LANE_BUNDLE_BYTES)
}

fn create_lane_bundle_with_limit(
    state: AppState,
    peer_id: String,
    request: MeshWorkerLaneBundleRequest,
    bundle_limit: u64,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let allowed = {
        let mesh = state.mesh.lock();
        mesh.paired_peers
            .get(&peer_id)
            .is_some_and(|peer| super::peer_has_full_access(Some(&peer.access_level)))
    };
    if !allowed {
        return Err((
            StatusCode::FORBIDDEN,
            "lane bundle requires Full mesh access".into(),
        ));
    }
    let lane_cwd = {
        let mux = state.svc.mux.read();
        let found = mux.all_tabs().find_map(|tab| {
            let binding = tab.task_binding.as_ref()?;
            (binding.plan_id.as_deref() == Some(&request.plan_id)
                && binding.task_id.as_deref() == Some(&request.task_id)
                && binding.attempt_id.as_deref() == Some(&request.attempt_id)
                && tab.report_target_mesh_peer.as_deref() == Some(peer_id.as_str()))
            .then(|| tab.provisioned_cwd.clone().or_else(|| tab.cwd.clone()))?
        });
        found
    };
    let checkout = lane_cwd.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            "remote attempt lane not found for authenticated origin".into(),
        )
    })?;
    if !codeterm_service_core::service::worker_lane::repository_matches(
        &checkout,
        &request.repository_identities,
        &request.repository_identity,
    ) {
        return Err((
            StatusCode::CONFLICT,
            "remote attempt repository identity mismatch".into(),
        ));
    }
    if !codeterm_service_core::service::worker_lane::contains_commit(
        &checkout,
        &request.base_commit,
    ) {
        return Err((
            StatusCode::CONFLICT,
            "peer_missing_base: base commit is absent".into(),
        ));
    }
    let reference = format!("refs/heads/{}", request.branch);
    let mut command = codeterm_util::process_util::git_command();
    let output = command
        .current_dir(&checkout)
        .args(["check-ref-format", &reference])
        .output()
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    if !output.status.success() || !request.branch.starts_with("lane/") {
        return Err((StatusCode::BAD_REQUEST, "invalid peer lane branch".into()));
    }
    if !codeterm_service_core::service::worker_lane::current_branch(&checkout)
        .is_ok_and(|branch| branch == request.branch.as_str())
    {
        return Err((
            StatusCode::CONFLICT,
            "attempt checkout is not on the requested lane branch".into(),
        ));
    }
    let lane_tip = codeterm_service_core::service::worker_lane::git_value(
        &checkout,
        ["rev-parse", &reference],
    )
    .map_err(|error| (StatusCode::CONFLICT, error))?;
    if lane_tip != request.report_sha {
        return Err((
            StatusCode::CONFLICT,
            "peer lane tip differs from accepted report SHA".into(),
        ));
    }
    if !codeterm_service_core::service::worker_lane::git_value(&checkout, ["rev-parse", "HEAD"])
        .is_ok_and(|head| head == request.report_sha.as_str())
    {
        return Err((
            StatusCode::CONFLICT,
            "attempt checkout HEAD differs from accepted report SHA".into(),
        ));
    }
    if !codeterm_service_core::service::worker_lane::is_ancestor(
        &checkout,
        &request.base_commit,
        &lane_tip,
    ) {
        return Err((
            StatusCode::CONFLICT,
            "peer lane does not descend from requested base".into(),
        ));
    }
    let temp = tempfile::tempdir()
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let bundle_path = temp.path().join("worker-lane.bundle");
    let mut command = codeterm_util::process_util::git_command();
    command
        .current_dir(&checkout)
        .args(["bundle", "create"])
        .arg(&bundle_path)
        .arg(&reference);
    if request.base_commit == lane_tip {
        command.arg("--max-count=1");
    } else {
        command.arg(format!("^{}", request.base_commit));
    }
    let output = command
        .output()
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    if !output.status.success() {
        return Err((
            StatusCode::CONFLICT,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    let size = std::fs::metadata(&bundle_path)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .len();
    if !lane_bundle_size_allowed(size, bundle_limit) {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            "worker lane bundle exceeds 32 MiB".into(),
        ));
    }
    let bytes = std::fs::read(&bundle_path)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    use base64::Engine;
    #[derive(Serialize)]
    struct BundleResponse {
        branch: String,
        base_commit: String,
        lane_tip: String,
        bundle: String,
    }
    Ok(Json(
        serde_json::to_value(BundleResponse {
            branch: request.branch,
            base_commit: request.base_commit,
            lane_tip,
            bundle: base64::engine::general_purpose::STANDARD.encode(bytes),
        })
        .unwrap(),
    ))
}

/// The origin's brief carries the remote ACK/report commands, so the peer tab holds no
/// local Worker role: that role's primer and placement assume a local Orchestrator.
pub(crate) fn remote_worker_spawn_params(
    request: MeshWorkerDispatchRequest,
    workspace_id: String,
    worktree_path: String,
    origin_peer_id: String,
    origin_peer_name: &str,
) -> codeterm_service_core::service::SpawnAgentParams {
    let suffix: String = request.attempt_id.chars().take(6).collect();
    codeterm_service_core::service::SpawnAgentParams {
        agent: Some(request.provider),
        cwd: Some(worktree_path.clone()),
        provisioned_cwd: Some(worktree_path),
        title: Some(format!("Remote{suffix}")),
        description: request.description,
        task_description_managed: request.task_description_managed,
        plan_id: Some(request.plan_id.clone()),
        task_id: Some(request.task_id.clone()),
        task_launch_authority: Some(codeterm_service_core::service::TaskLaunchAuthority {
            plan_id: request.plan_id.clone(),
            task_id: request.task_id.clone(),
            attempt_id: request.attempt_id.clone(),
            lane_base: Some(request.base_commit.clone()),
        }),
        execution_intent: Some(codeterm_service_core::plan::ExecutionIntent::Mutating),
        workspace_id: Some(workspace_id),
        task: Some(request.brief),
        ownership: Some(codeterm_types::pty::RemoteOwnership {
            controller_title: request.origin_tab_id.clone(),
            controller_tab_id: Some(request.origin_tab_id.clone()),
            peer_id: origin_peer_id.clone(),
            peer_name: origin_peer_name.into(),
            team: None,
        }),
        report_target_tab_id: Some(request.origin_tab_id),
        report_target_mesh_peer: Some(origin_peer_id.clone()),
        remote_task_binding: Some(codeterm_service_core::service::RemoteTaskBinding {
            plan_id: request.plan_id,
            task_id: request.task_id,
            attempt_id: request.attempt_id,
            origin_peer_id,
        }),
        model: request.model,
        speed: request.speed,
        reasoning_effort: request.reasoning_effort,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests;

pub async fn mesh_worker_dispatch(
    AxumState(state): AxumState<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<MeshWorkerDispatchRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let origin_peer_id = super::relay::check_mesh_relay_auth(&state, &headers)?;
    tokio::task::spawn_blocking(move || {
        dispatch_authenticated_worker(state, origin_peer_id, request)
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

fn dispatch_authenticated_worker(
    state: AppState,
    origin_peer_id: String,
    request: MeshWorkerDispatchRequest,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let (origin_peer_name, origin_peer_id, local_peer_name) = {
        let mesh = state.mesh.lock();
        let Some(peer) = mesh.paired_peers.get(&origin_peer_id) else {
            return Err((StatusCode::UNAUTHORIZED, "origin peer is not paired".into()));
        };
        if peer.access_level != codeterm_service_core::mesh::types::AccessLevel::Full {
            return Err((
                StatusCode::FORBIDDEN,
                "remote worker dispatch requires Full mesh access".into(),
            ));
        }
        (
            peer.instance_name.clone(),
            origin_peer_id,
            mesh.ident.identity.instance_name.clone(),
        )
    };
    let workspaces = {
        let mux = state.svc.mux.read();
        mux.list_workspaces()
            .into_iter()
            .filter_map(|workspace| workspace.cwd.map(|cwd| (workspace.id, cwd)))
            .collect::<Vec<_>>()
    };
    let matched = workspaces.into_iter().find_map(|(workspace_id, cwd)| {
        let primary = codeterm_service_core::service::worker_lane::primary_checkout(
            std::path::Path::new(&cwd),
        )?;
        let canonical_cwd = primary.to_string_lossy().to_string();
        let remote = codeterm_service_core::service::worker_lane::matching_remote_for_identity(
            &canonical_cwd,
            &request.repository_identities,
            &request.repository_identity,
        )?;
        Some((workspace_id, canonical_cwd, remote))
    });
    let Some((workspace_id, canonical_cwd, matching_remote)) = matched else {
        let clone = request.clone_url.as_deref().map_or_else(
            || "the dispatch named no clone URL".to_string(),
            |url| super::template_hosting::ask_clone(&state, &origin_peer_id, url),
        );
        return Err((
            StatusCode::CONFLICT,
            format!("peer_missing_repo: no checkout identity matches the dispatching peer; nothing was cloned ({clone}). Choose another placement (another --mesh peer or local dispatch); older peers may require matching remote URLs"),
        ));
    };
    if !codeterm_service_core::service::worker_lane::contains_commit(
        &canonical_cwd,
        &request.base_commit,
    ) {
        if request.repository_identities.is_empty() {
            return Err((
                StatusCode::CONFLICT,
                "peer_missing_base: this dispatch did not identify a matching remote for fetching; update both peers".into(),
            ));
        }
        codeterm_service_core::service::worker_lane::fetch_base_commit(
            &canonical_cwd,
            &matching_remote,
            &request.base_commit,
        )
        .map_err(|error| {
            (
                StatusCode::CONFLICT,
                format!("peer_missing_base: requested commit is absent and fetch failed: {error}"),
            )
        })?;
        if !codeterm_service_core::service::worker_lane::contains_commit(
            &canonical_cwd,
            &request.base_commit,
        ) {
            return Err((
                StatusCode::CONFLICT,
                "peer_missing_base: matching remote fetch completed without the requested commit"
                    .into(),
            ));
        }
    }
    preflight_remote_worker_provider(
        &state.svc,
        &request.provider,
        &workspace_id,
        &local_peer_name,
    )?;
    let lane_label = format!("{}-{}", request.plan_id, request.task_id);
    let lane = codeterm_service_core::service::worker_lane::provision_isolated_lane_at_base(
        &canonical_cwd,
        &lane_label,
        &request.base_commit,
        &state.svc.worker_lane_settings(),
    )
    .map_err(|error| {
        (
            StatusCode::CONFLICT,
            format!("peer_lane_provision_failed: {error}"),
        )
    })?;
    let provenance = match lane.verify() {
        Ok(provenance) => provenance,
        Err(error) => {
            let cleanup = lane.cleanup();
            return Err((
                StatusCode::CONFLICT,
                match cleanup {
                    Ok(()) => format!("peer_lane_verification_failed: {error}"),
                    Err(cleanup) => {
                        format!("peer_lane_verification_failed: {error}; cleanup failed: {cleanup}")
                    }
                },
            ));
        }
    };
    let lease_holder = format!("attempt:{}", request.attempt_id);
    let keepawake = codeterm_service_core::service::team_template_remote::acquire_hosting_lease(
        &state.svc,
        &lease_holder,
    );
    let spawn = remote_worker_spawn_params(
        request.clone(),
        workspace_id,
        provenance.worktree_path.clone(),
        origin_peer_id,
        &origin_peer_name,
    );
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let result = match state.svc.spawn_agent_sync_with_ready(spawn, Some(ready_tx)) {
        Ok(result) => result,
        Err(error) => {
            codeterm_service_core::service::team_template_remote::release_hosting_lease(
                &lease_holder,
            );
            let cleanup = lane.cleanup();
            return Err(classify_spawn_failure(
                &state.svc,
                &request.provider,
                &local_peer_name,
                &error,
                cleanup,
            ));
        }
    };
    codeterm_service_core::service::team_template_remote::HostingLeases::global()
        .transfer(&lease_holder, &result.tab_id);
    if let Err(error) = await_remote_launch_outcome(
        &state.svc,
        &result.tab_id,
        ready_rx,
        REMOTE_LAUNCH_READY_WAIT,
    ) {
        codeterm_service_core::service::team_template_remote::release_hosting_lease(&result.tab_id);
        let _ = state.svc.close_tab_with_reason(
            &result.tab_id,
            codeterm_service_core::multiplexer::types::TabCloseReason::SpawnRollback,
        );
        let cleanup = lane.cleanup();
        return Err(classify_spawn_failure(
            &state.svc,
            &request.provider,
            &local_peer_name,
            &error,
            cleanup,
        ));
    }
    let worker_tab = state
        .svc
        .mux
        .read()
        .get_tab(&result.tab_id)
        .map(|tab| tab.session_binding_nonce.clone());
    // A Worker tab that closed before the lease moved onto it never sees its own close.
    if worker_tab.is_none() {
        codeterm_service_core::service::team_template_remote::release_hosting_lease(&result.tab_id);
    }
    let nonce = worker_tab.flatten();
    Ok(Json(serde_json::json!({
        "session_binding_nonce": nonce,
        "keepawake": keepawake,
        "state": "awaiting_worker_ack",
        "peer_id": state.mesh.lock().ident.identity.instance_id,
        "worker_tab_id": result.tab_id,
        "title": result.title,
        "branch": provenance.branch,
        "worktree_path": provenance.worktree_path,
        "base_commit": provenance.base,
        "repository_identity": request.repository_identity,
    })))
}

pub async fn mesh_worker_dispatch_status(
    AxumState(state): AxumState<AppState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<MeshWorkerDispatchRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let peer_id = super::relay::check_mesh_relay_auth(&state, &headers)?;
    tokio::task::spawn_blocking(move || observed_worker_dispatch(&state, &peer_id, &request))
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

fn observed_worker_dispatch(
    state: &AppState,
    peer_id: &str,
    request: &MeshWorkerDispatchRequest,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if !state
        .mesh
        .lock()
        .paired_peers
        .get(peer_id)
        .is_some_and(|peer| super::peer_has_full_access(Some(&peer.access_level)))
    {
        return Err((
            StatusCode::FORBIDDEN,
            "worker dispatch status requires Full mesh access".into(),
        ));
    }
    let worker = {
        let mux = state.svc.mux.read();
        let found = mux.all_tabs().find_map(|tab| {
            let binding = tab.task_binding.as_ref()?;
            if binding.plan_id.as_deref() != Some(&request.plan_id)
                || binding.task_id.as_deref() != Some(&request.task_id)
                || binding.attempt_id.as_deref() != Some(&request.attempt_id)
                || tab.report_target_mesh_peer.as_deref() != Some(peer_id)
            {
                return None;
            }
            Some((
                tab.id.clone(),
                tab.provisioned_cwd.clone().or_else(|| tab.cwd.clone())?,
                tab.session_binding_nonce.clone()?,
            ))
        });
        found
    };
    let (tab_id, cwd, nonce) = worker.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            "remote worker attempt not found".into(),
        )
    })?;
    if !codeterm_service_core::service::worker_lane::repository_matches(
        &cwd,
        &request.repository_identities,
        &request.repository_identity,
    ) || !codeterm_service_core::service::worker_lane::contains_commit(
        &cwd,
        &request.base_commit,
    ) {
        return Err((
            StatusCode::CONFLICT,
            "remote worker lane does not match dispatch".into(),
        ));
    }
    let branch = codeterm_service_core::service::worker_lane::current_branch(&cwd)
        .map_err(|error| (StatusCode::CONFLICT, error))?;
    Ok(Json(serde_json::json!({
        "worker_tab_id": tab_id,
        "session_binding_nonce": nonce,
        "state": "awaiting_worker_ack",
        "branch": branch,
        "worktree_path": cwd,
        "base_commit": request.base_commit,
    })))
}
