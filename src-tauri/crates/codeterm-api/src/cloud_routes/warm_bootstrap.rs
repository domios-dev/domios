use axum::{
    extract::{Path, State as AxumState},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    Json,
};
use codeterm_service_core::{
    mesh::types::{AccessLevel, RelayCommand},
    service::{
        cloud_machines::{
            warm_bootstrap::{
                apply_portable_preferences_to_config, apply_warm_bootstrap, build_owner_projection,
                inspect_provider_readiness, prepare_provider_signin_tabs,
                read_verified_recovery_revision, HomeExportInput, PrepareProviderSignInRequest,
                PrepareProviderSignInResponse, ProviderSignInCardStatus, TargetProvenance,
                WarmBootstrapApplyEnv, WarmBootstrapApplyResult, WarmBootstrapProjection,
                WarmBootstrapRejectReason, WarmBootstrapStateStore, WarmBootstrapStatus,
            },
            CloudError, CloudMachineRecord,
        },
        general_agent,
    },
};
use serde::Deserialize;

use super::adapters::MeshPeerControlAdapter;
use super::{error_response, require_local_caller, require_local_owner};
use crate::auth::{authenticated_request, AuthenticatedOrigin, CallerIdentity};
use crate::routes::AppState;

fn no_store_json(
    status: StatusCode,
    value: serde_json::Value,
) -> (StatusCode, HeaderMap, Json<serde_json::Value>) {
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    (status, headers, Json(value))
}

fn require_full_peer_owner(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    match authenticated_request(state, headers) {
        Ok((
            CallerIdentity::Peer {
                instance_id,
                access_level: AccessLevel::Full,
                ..
            },
            AuthenticatedOrigin::Owner | AuthenticatedOrigin::Mesh,
        )) => {
            let mesh = state.mesh.lock();
            let caller_installation = mesh
                .link_record(&instance_id)
                .and_then(|link| link.installation_id())
                .map(str::to_owned)
                .unwrap_or(instance_id);
            Ok(caller_installation)
        }
        _ => Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "ok": false,
                "error": {
                    "code": "warm_bootstrap_forbidden",
                    "message": "Warm bootstrap peer routes require an authenticated same-account Full-access peer with owner origin",
                }
            })),
        )),
    }
}

fn target_identity(state: &AppState) -> (String, String) {
    let mesh = state.mesh.lock();
    let account_id = mesh
        .account_state()
        .signed_in_account()
        .map(str::to_owned)
        .unwrap_or_default();
    let target_installation_id = mesh
        .account_state()
        .membership
        .as_ref()
        .map(|m| m.verified.installation_id.clone())
        .unwrap_or_else(|| mesh.status().instance_id.clone());
    (account_id, target_installation_id)
}

fn configured_providers_from_config(state: &AppState) -> Vec<String> {
    let cfg = state.config.lock().clone();
    let mut ids = Vec::new();
    let mut push = |id: &str| {
        let trimmed = id.trim();
        if !trimmed.is_empty() && !ids.iter().any(|existing| existing == trimmed) {
            ids.push(trimmed.to_owned());
        }
    };
    push(&cfg.general_agent.agent_provider);
    let mut keys: Vec<String> = cfg.agents.keys().cloned().collect();
    keys.sort();
    for k in keys {
        push(&k);
    }
    ids
}

fn ensure_warm_general_console(state: &AppState) -> Option<String> {
    let cfg = state.config.lock().clone();
    let provider_id = cfg.general_agent.agent_provider.clone();
    let default_chat_view = general_agent::default_chat_view_for_provider(
        &cfg,
        &state.svc.agent_registry,
        &provider_id,
    );
    let mut mux = state.svc.mux.write();
    general_agent::ensure_general_workspace(&mut mux, default_chat_view);
    mux.resolve_tab_id(general_agent::GENERAL_ALIAS)
}

/// Target peer endpoint: applies a bounded, versioned owner projection once on a truly empty
/// fresh cloud machine, preserving target-owned authority and verified recovery revisions.
pub(super) async fn peer_warm_bootstrap(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(projection): Json<WarmBootstrapProjection>,
) -> Result<(StatusCode, HeaderMap, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)>
{
    let caller_installation_id = require_full_peer_owner(&state, &headers)?;
    let (enrolled_account_id, target_installation_id) = target_identity(&state);
    let state_clone = state.clone();

    let result = tokio::task::spawn_blocking(move || {
        let warm_mesh_dir = state_clone.svc.cloud_machines().warm_state_dir();
        let data_root = warm_mesh_dir
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(codeterm_paths::data_dir);
        let state_store =
            WarmBootstrapStateStore::new(warm_mesh_dir.join("cloud_warm_bootstrap.json"));
        let general_dir = data_root.join("general");
        let verified_recovery_revision = read_verified_recovery_revision(&data_root);
        let configured_provider_ids: Vec<String> = projection
            .preferences
            .configured_providers
            .iter()
            .map(|p| p.provider_id.clone())
            .collect();
        let configured_profile_ids: Vec<String> = projection
            .preferences
            .acp_profiles
            .iter()
            .map(|p| p.profile_id.clone())
            .collect();
        let now_ms = state_clone.svc.cloud_machines().now_ms();
        let general_tab_id = {
            let mux = state_clone.svc.mux.read();
            mux.resolve_tab_id(general_agent::GENERAL_ALIAS)
        };

        let provenance = TargetProvenance {
            enrolled_account_id: &enrolled_account_id,
            target_installation_id: &target_installation_id,
            authenticated_caller_installation_id: &caller_installation_id,
            verified_recovery_revision: verified_recovery_revision.as_deref(),
        };
        let env = WarmBootstrapApplyEnv {
            mem: &state_clone.svc.mem,
            general_dir: &general_dir,
            state_store: &state_store,
            now_ms,
            general_tab_id,
            providers: Vec::new(),
        };

        let mut applied = apply_warm_bootstrap(
            projection,
            &provenance,
            &env,
            |prefs| {
                let mut cfg = state_clone.config.lock();
                let count = apply_portable_preferences_to_config(&mut cfg, prefs);
                cfg.general_agent.first_run_complete = true;
                Ok(count)
            },
            || {
                let _ = ensure_warm_general_console(&state_clone);
                if general_dir == general_agent::general_dir() {
                    let cfg = state_clone.config.lock().general_agent.clone();
                    general_agent::generate_instructions(
                        cfg.capability_hints,
                        &cfg.agent_provider,
                        Some(&state_clone.svc.mem),
                    );
                }
                Ok(())
            },
        );

        if applied.status != WarmBootstrapStatus::Rejected {
            applied.general_tab_id = ensure_warm_general_console(&state_clone);
            let effective_providers = if configured_provider_ids.is_empty() {
                configured_providers_from_config(&state_clone)
            } else {
                configured_provider_ids
            };
            applied.providers = inspect_provider_readiness(
                &state_clone.svc,
                &effective_providers,
                &configured_profile_ids,
            );
        }
        applied
    })
    .await
    .map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "ok": false,
                "error": {
                    "code": "warm_bootstrap_task_failed",
                    "message": error.to_string(),
                }
            })),
        )
    })?;

    let status_code = match (result.status, result.reject_reason) {
        (
            WarmBootstrapStatus::Rejected,
            Some(
                WarmBootstrapRejectReason::WrongAccount
                | WarmBootstrapRejectReason::InstallationMismatch
                | WarmBootstrapRejectReason::InvalidProvenance,
            ),
        ) => StatusCode::FORBIDDEN,
        (
            WarmBootstrapStatus::Rejected,
            Some(
                WarmBootstrapRejectReason::StaleGeneration
                | WarmBootstrapRejectReason::TargetNotEmpty,
            ),
        ) => StatusCode::CONFLICT,
        (
            WarmBootstrapStatus::Rejected,
            Some(
                WarmBootstrapRejectReason::UnsupportedSchema
                | WarmBootstrapRejectReason::OversizedProjection,
            ),
        ) => StatusCode::BAD_REQUEST,
        _ => StatusCode::OK,
    };

    Ok(no_store_json(
        status_code,
        serde_json::to_value(result).unwrap_or_default(),
    ))
}

/// Target peer endpoint: returns non-secret per-provider readiness and supported login capabilities.
pub(super) async fn peer_provider_readiness(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, HeaderMap, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)>
{
    let _caller = require_full_peer_owner(&state, &headers)?;
    let state_clone = state.clone();
    let providers: Vec<ProviderSignInCardStatus> = tokio::task::spawn_blocking(move || {
        let configured = configured_providers_from_config(&state_clone);
        let acp_profiles: Vec<String> = codeterm_plugin::agent_runtime::owner_profiles::load("acp")
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        inspect_provider_readiness(&state_clone.svc, &configured, &acp_profiles)
    })
    .await
    .map_err(|error| error_response(CloudError::Internal(error.to_string())))?;

    Ok(no_store_json(
        StatusCode::OK,
        serde_json::json!({ "providers": providers }),
    ))
}

/// Target peer endpoint: prepares sign-in card bindings for signed-out supported providers.
/// Never accepts or returns secrets, codes, or credentials.
pub(super) async fn peer_prepare_provider_signin(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(request): Json<PrepareProviderSignInRequest>,
) -> Result<(StatusCode, HeaderMap, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)>
{
    let _caller = require_full_peer_owner(&state, &headers)?;
    let state_clone = state.clone();
    let reply = tokio::task::spawn_blocking(move || {
        let configured = configured_providers_from_config(&state_clone);
        let acp_profiles: Vec<String> = codeterm_plugin::agent_runtime::owner_profiles::load("acp")
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        prepare_provider_signin_tabs(&state_clone.svc, &configured, &acp_profiles, &request, None)
    })
    .await
    .map_err(|error| error_response(CloudError::Internal(error.to_string())))?;

    Ok(no_store_json(
        StatusCode::OK,
        serde_json::to_value(reply).unwrap_or_default(),
    ))
}

/// Home controller endpoint: returns sanitized warm-bootstrap status, phase timings, and provider readiness.
pub(super) async fn machine_bootstrap_status(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<(StatusCode, HeaderMap, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)>
{
    require_local_caller(&state, &headers)?;
    let ctrl = state.svc.cloud_machines();
    let record = ctrl
        .find_by_peer(&id)
        .ok_or_else(|| error_response(CloudError::MachineNotFound(id.clone())))?;
    let entry = ctrl
        .bootstrap_status(&record.machine_id)
        .map_err(error_response)?;
    let peer_id = record
        .peer_instance_id
        .clone()
        .or_else(|| record.installation_id.clone())
        .unwrap_or_else(|| record.machine_id.clone());

    Ok(no_store_json(
        StatusCode::OK,
        serde_json::json!({
            "machine_id": record.machine_id,
            "name": record.name,
            "state": record.state,
            "peer_id": peer_id,
            "installation_id": record.installation_id,
            "creation_generation": entry.creation_generation,
            "recovery_revision": entry.recovery_revision,
            "create_phases": entry.create_phases,
            "warm_bootstrap": entry.warm_bootstrap,
        }),
    ))
}

/// Home controller endpoint: callable by local Owner or authorized local Agent to request
/// sign-in card bindings on a cloud machine. Agents receive ONLY typed capability/status metadata;
/// owner-entered secret input remains flow-bound to the owner UI.
pub(super) async fn prepare_machine_provider_signin(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<PrepareProviderSignInRequest>,
) -> Result<(StatusCode, HeaderMap, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)>
{
    require_local_caller(&state, &headers)?;
    let ctrl = state.svc.cloud_machines();
    let record = ctrl
        .find_by_peer(&id)
        .ok_or_else(|| error_response(CloudError::MachineNotFound(id.clone())))?;
    let peer_ctrl = MeshPeerControlAdapter::new(state.clone());
    let response = ctrl
        .prepare_provider_signin(&record.machine_id, &request, &peer_ctrl)
        .await
        .map_err(error_response)?;
    Ok(no_store_json(
        StatusCode::OK,
        serde_json::to_value(response).unwrap_or_default(),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProviderSignInTimingBody {
    #[serde(default)]
    pub started_at_ms: Option<u64>,
    #[serde(default)]
    pub completed_at_ms: Option<u64>,
}

/// Home controller endpoint: local owner records human provider sign-in timing separately
/// from automated warm-bootstrap duration.
pub(super) async fn record_provider_signin_timing(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ProviderSignInTimingBody>,
) -> Result<(StatusCode, HeaderMap, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)>
{
    require_local_owner(&state, &headers)?;
    let ctrl = state.svc.cloud_machines();
    let record = ctrl
        .find_by_peer(&id)
        .ok_or_else(|| error_response(CloudError::MachineNotFound(id.clone())))?;
    let phases = ctrl
        .record_provider_signin_timing(&record.machine_id, body.started_at_ms, body.completed_at_ms)
        .map_err(error_response)?;
    Ok(no_store_json(
        StatusCode::OK,
        serde_json::json!({
            "machine_id": record.machine_id,
            "create_phases": phases,
        }),
    ))
}

pub(super) async fn relay_apply_warm_bootstrap(
    adapter: &MeshPeerControlAdapter,
    record: &CloudMachineRecord,
    creation_generation: &str,
) -> Result<Option<WarmBootstrapApplyResult>, CloudError> {
    let target_installation_id = match adapter.resolve_installation_id(record) {
        Some(id) if !id.trim().is_empty() => id,
        _ => return Ok(None),
    };
    let (account_id, owner_installation_id) = {
        let mesh = adapter.state.mesh.lock();
        let Some(account_id) = mesh.account_state().signed_in_account().map(str::to_owned) else {
            return Ok(None);
        };
        let owner_id = mesh
            .account_state()
            .membership
            .as_ref()
            .map(|m| m.verified.installation_id.clone())
            .unwrap_or_else(|| mesh.status().instance_id.clone());
        (account_id, owner_id)
    };

    let state_clone = adapter.state.clone();
    let generation_owned = creation_generation.to_owned();
    let target_id_clone = target_installation_id.clone();
    let (projection, export_exclusions) = tokio::task::spawn_blocking(move || {
        let cfg = state_clone.config.lock().clone();
        let acp_profiles =
            codeterm_plugin::agent_runtime::owner_profiles::load("acp").unwrap_or_default();
        let available_provider_ids = state_clone.svc.agent_registry.provider_ids();
        let general_dir = general_agent::general_dir();
        build_owner_projection(&HomeExportInput {
            account_id: &account_id,
            owner_installation_id: &owner_installation_id,
            target_installation_id: &target_id_clone,
            creation_generation: &generation_owned,
            recovery_revision: None,
            mem: &state_clone.svc.mem,
            general_dir: &general_dir,
            config: &cfg,
            acp_owner_profiles: &acp_profiles,
            available_provider_ids: &available_provider_ids,
        })
    })
    .await
    .map_err(|error| CloudError::Internal(error.to_string()))?;

    let body = serde_json::to_value(&projection)
        .map_err(|error| CloudError::Internal(error.to_string()))?;
    let cmd = RelayCommand {
        method: "POST".into(),
        path: "/api/mesh/cloud/peer/warm-bootstrap".into(),
        body,
        delivery_id: None,
    };

    let value = if let Some(cloud_result) = adapter.send_controller_cloud_relay(record, cmd).await {
        let resp =
            cloud_result.map_err(|error| CloudError::ProviderUnavailable(error.to_string()))?;
        if let Some(data) = resp.data {
            data
        } else {
            return Err(CloudError::ProviderUnavailable(resp.error.unwrap_or_else(
                || format!("cloud warm bootstrap returned status {}", resp.status),
            )));
        }
    } else {
        let client = adapter.peer_client(record)?;
        client
            .auth_post_json("/api/mesh/cloud/peer/warm-bootstrap", &projection)
            .await
            .map_err(CloudError::ProviderUnavailable)?
    };

    let mut result: WarmBootstrapApplyResult =
        serde_json::from_value(value).map_err(|error| CloudError::Internal(error.to_string()))?;
    if !export_exclusions.is_empty() {
        result.excluded.extend(export_exclusions);
        if result.status == WarmBootstrapStatus::Applied {
            result.status = WarmBootstrapStatus::Partial;
        }
    }
    Ok(Some(result))
}

pub(super) async fn relay_prepare_provider_signin(
    adapter: &MeshPeerControlAdapter,
    record: &CloudMachineRecord,
    request: &PrepareProviderSignInRequest,
) -> Result<PrepareProviderSignInResponse, CloudError> {
    let body =
        serde_json::to_value(request).map_err(|error| CloudError::Internal(error.to_string()))?;
    let cmd = RelayCommand {
        method: "POST".into(),
        path: "/api/mesh/cloud/peer/provider-signin/prepare".into(),
        body,
        delivery_id: None,
    };

    let value = if let Some(cloud_result) = adapter.send_controller_cloud_relay(record, cmd).await {
        let resp =
            cloud_result.map_err(|error| CloudError::ProviderUnavailable(error.to_string()))?;
        if !resp.ok {
            return Err(CloudError::ProviderUnavailable(resp.error.unwrap_or_else(
                || format!("provider sign-in prepare returned status {}", resp.status),
            )));
        }
        resp.data.unwrap_or(serde_json::Value::Null)
    } else {
        let client = adapter.peer_client(record)?;
        client
            .auth_post_json("/api/mesh/cloud/peer/provider-signin/prepare", request)
            .await
            .map_err(CloudError::ProviderUnavailable)?
    };

    let mut reply: PrepareProviderSignInResponse =
        serde_json::from_value(value).map_err(|error| CloudError::Internal(error.to_string()))?;
    let peer_prefix = record
        .peer_instance_id
        .as_deref()
        .or(record.installation_id.as_deref())
        .unwrap_or(&record.machine_id);
    for item in &mut reply.providers {
        if item.remote_tab_id.is_none() {
            if let Some(tab_id) = item.tab_id.as_deref() {
                item.remote_tab_id = Some(format!("mesh:{peer_prefix}/{tab_id}"));
            }
        }
    }
    Ok(reply)
}
