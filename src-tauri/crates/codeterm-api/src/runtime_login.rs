use super::routes::AppState;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use codeterm_service_core::service::agent_runtime::login::{operate, LoginRequest};
use codeterm_service_core::service::background_work::{self, BackgroundWorkTarget};

pub(super) async fn tab_usage(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(tab): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    codeterm_service_core::service::agent_runtime::read_usage_for_tab(&state.svc, &tab)
        .await
        .map(Json)
        .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn background_processes(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(tab): Path<String>,
    axum::extract::Query(request): axum::extract::Query<
        codeterm_service_core::service::agent_runtime::background::BackgroundProcessAuthority,
    >,
) -> Result<
    Json<codeterm_service_core::service::agent_runtime::background::BackgroundProcessSnapshot>,
    (StatusCode, String),
> {
    codeterm_service_core::service::agent_runtime::background::list_for_tab(
        &state.svc, &tab, request,
    )
    .await
    .map(Json)
    .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn stop_background_process(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<
        codeterm_service_core::service::agent_runtime::background::StopBackgroundProcessRequest,
    >,
) -> Result<
    Json<codeterm_service_core::service::agent_runtime::background::BackgroundStopResult>,
    (StatusCode, String),
> {
    codeterm_service_core::service::agent_runtime::background::stop_for_tab(
        &state.svc, &tab, request,
    )
    .await
    .map(Json)
    .map_err(|reason| (StatusCode::CONFLICT, reason))
}

async fn provider_background_output(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
    axum::extract::Query(target): axum::extract::Query<BackgroundWorkTarget>,
) -> Result<
    Json<codeterm_service_core::service::local_preview::html_preview::LocalPreviewFile>,
    (StatusCode, String),
> {
    require_background_work_authority(&state, &headers)?;
    background_work_action(move || {
        let configured = state.config.lock().api.preview_secret_paths.clone();
        background_work::read_output(&state.svc, &tab, &configured, &target)
    })
    .await
}

async fn ask_provider_to_stop_background_work(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
    Json(target): Json<BackgroundWorkTarget>,
) -> Result<Json<&'static str>, (StatusCode, String)> {
    require_background_work_authority(&state, &headers)?;
    background_work_action(move || background_work::ask_stop(&state.svc, &tab, target)).await
}

fn require_background_work_authority(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, String)> {
    match crate::auth::RequestActor::authenticate(state, headers)? {
        crate::auth::RequestActor::Owner | crate::auth::RequestActor::Peer => Ok(()),
        _ => Err((
            StatusCode::FORBIDDEN,
            "Background process controls require the owner".into(),
        )),
    }
}

async fn background_work_action<T: Send + 'static>(
    action: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<Json<T>, (StatusCode, String)> {
    tokio::task::spawn_blocking(action)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Background process operation failed".to_string(),
            )
        })?
        .map(Json)
        .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn child_history(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path((tab, thread)): Path<(String, String)>,
    axum::extract::Query(request): axum::extract::Query<
        codeterm_service_core::service::agent_runtime::children::ChildHistoryRequest,
    >,
) -> Result<
    Json<codeterm_service_core::service::agent_runtime::children::ChildHistoryPage>,
    (StatusCode, String),
> {
    codeterm_service_core::service::agent_runtime::children::history(
        &state.svc, &tab, &thread, request,
    )
    .await
    .map(Json)
    .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn show_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
) -> Result<(HeaderMap, Json<serde_json::Value>), (StatusCode, String)> {
    require_permission_authority(&state, &headers)?;
    let installation_id = state.mesh.lock().ident.identity.instance_id.clone();
    codeterm_service_core::service::agent_runtime::login::show(&state.svc, &tab)
        .map(|mut value| {
            stamp_binding_installation_id(&mut value, &installation_id);
            login_response(value)
        })
        .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<LoginRequest>,
) -> Result<(HeaderMap, Json<serde_json::Value>), (StatusCode, String)> {
    let is_code_request = request.action.as_deref() == Some("code") || request.code.is_some();
    let installation_id = state.mesh.lock().ident.identity.instance_id.clone();
    if is_code_request {
        let is_remote =
            require_code_submission_authority(&state, &headers).map_err(|(status, reason)| {
                (status, typed_login_refusal("login_forbidden", &reason))
            })?;
        validate_code_route_binding(&installation_id, &tab, is_remote, &request).map_err(
            |reason| {
                (
                    StatusCode::CONFLICT,
                    typed_login_refusal("login_binding_mismatch", &reason),
                )
            },
        )?;
    } else {
        require_permission_authority(&state, &headers)?;
    }
    if request.api_key.is_some() && !super::auth::is_local_caller(&state, &headers) {
        return Err((
            StatusCode::FORBIDDEN,
            "API keys must be entered on the provider's own machine".into(),
        ));
    }
    operate(&state.svc, &tab, request)
        .await
        .map(|mut value| {
            stamp_binding_installation_id(&mut value, &installation_id);
            login_response(value)
        })
        .map_err(|reason| {
            if is_code_request {
                (
                    StatusCode::CONFLICT,
                    typed_login_refusal("login_refused", &reason),
                )
            } else {
                (StatusCode::CONFLICT, reason)
            }
        })
}

pub(super) async fn reset_credits(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<
        codeterm_service_core::service::agent_runtime::reset_credits::ResetCreditsRequest,
    >,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    codeterm_service_core::service::agent_runtime::reset_credits::operate(&state.svc, &tab, request)
        .await
        .map(Json)
        .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn goal(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<codeterm_service_core::service::agent_runtime::goals::GoalRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    codeterm_service_core::service::agent_runtime::goals::operate(&state.svc, &tab, request)
        .await
        .map(Json)
        .map_err(|reason| (StatusCode::CONFLICT, reason))
}

pub(super) async fn thread_history(
    State(state): State<AppState>,
    _headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<
        codeterm_service_core::service::agent_runtime::thread_history::ThreadHistoryRequest,
    >,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    codeterm_service_core::service::agent_runtime::thread_history::operate(
        &state.svc, &tab, request,
    )
    .await
    .map(Json)
    .map_err(|reason| (StatusCode::CONFLICT, reason))
}

/// Permission choices and runtime sign-in controls belong to the local owner or a paired
/// `AccessLevel::Full` peer acting as owner/mesh, never to an agent or a narrowed/custom peer.
pub(crate) fn require_permission_authority(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, String)> {
    match crate::auth::authenticated_request(state, headers)? {
        (crate::auth::CallerIdentity::Local, crate::auth::AuthenticatedOrigin::Owner) => Ok(()),
        (
            crate::auth::CallerIdentity::Peer {
                access_level: codeterm_service_core::mesh::types::AccessLevel::Full,
                ..
            },
            crate::auth::AuthenticatedOrigin::Owner | crate::auth::AuthenticatedOrigin::Mesh,
        ) => Ok(()),
        _ => Err((
            StatusCode::FORBIDDEN,
            "Only the owner or a Full-access peer changes tab permissions or runtime login"
                .to_string(),
        )),
    }
}

/// Authorization-code submission requires strictly Owner origin: either the local human Owner
/// or a paired `AccessLevel::Full` peer carrying `AuthenticatedOrigin::Owner`. Agents, tab
/// credentials, workers, narrowed peers, and non-owner mesh origins are refused.
fn require_code_submission_authority(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<bool, (StatusCode, String)> {
    match crate::auth::authenticated_request(state, headers)? {
        (crate::auth::CallerIdentity::Local, crate::auth::AuthenticatedOrigin::Owner) => Ok(false),
        (
            crate::auth::CallerIdentity::Peer {
                access_level: codeterm_service_core::mesh::types::AccessLevel::Full,
                ..
            },
            crate::auth::AuthenticatedOrigin::Owner,
        ) => Ok(true),
        _ => Err((
            StatusCode::FORBIDDEN,
            "Only the owner or a Full-access peer with Owner origin may submit a sign-in code"
                .to_string(),
        )),
    }
}

fn validate_code_route_binding(
    installation_id: &str,
    tab: &str,
    is_remote: bool,
    request: &LoginRequest,
) -> Result<(), String> {
    if request
        .login_id
        .as_deref()
        .is_none_or(|id| id.trim().is_empty())
    {
        return Err("Sign-in code requires an active login flow id".into());
    }
    let binding = request
        .binding
        .as_ref()
        .ok_or("Sign-in code submission requires an active login binding")?;
    if let Some(provided) = binding.installation_id.as_deref() {
        if provided != installation_id {
            return Err("Sign-in code belongs to a different installation".into());
        }
    } else if is_remote {
        return Err("Remote sign-in code requires an exact installation binding".into());
    }
    if let Some(provided) = binding.tab_id.as_deref() {
        if provided != tab {
            return Err("Sign-in code belongs to a different tab".into());
        }
    } else if is_remote {
        return Err("Remote sign-in code requires an exact tab binding".into());
    }
    if is_remote
        && binding
            .provider_id
            .as_deref()
            .or(request.provider_id.as_deref())
            .is_none_or(|id| id.trim().is_empty())
    {
        return Err("Remote sign-in code requires an exact provider binding".into());
    }
    Ok(())
}

fn stamp_binding_installation_id(value: &mut serde_json::Value, installation_id: &str) {
    if installation_id.trim().is_empty() {
        return;
    }
    let Some(root) = value.as_object_mut() else {
        return;
    };
    if let Some(binding) = root
        .get_mut("loginBinding")
        .and_then(serde_json::Value::as_object_mut)
    {
        binding
            .entry("installationId".to_string())
            .or_insert_with(|| serde_json::Value::String(installation_id.to_owned()));
    }
    if let Some(binding) = root
        .get_mut("state")
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|state| state.get_mut("loginBinding"))
        .and_then(serde_json::Value::as_object_mut)
    {
        binding
            .entry("installationId".to_string())
            .or_insert_with(|| serde_json::Value::String(installation_id.to_owned()));
    }
}

fn typed_login_refusal(code: &str, reason: &str) -> String {
    serde_json::json!({
        "outcome": "refused",
        "reason": reason,
        "message": reason,
        "error": {
            "code": code,
            "message": reason,
            "bytes_written": false,
        }
    })
    .to_string()
}

async fn controls(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab): Path<String>,
    Json(request): Json<
        codeterm_service_core::service::agent_runtime::declared_controls::Selection,
    >,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    use codeterm_service_core::service::agent_runtime::declared_controls::{
        select, ControlError, RUNTIME_BUSY_MESSAGE, RUNTIME_BUSY_REASON,
    };
    if state
        .svc
        .runtime_control_requires_owner(&tab, request.mode, &request.id)
    {
        require_permission_authority(&state, &headers)?;
    }
    select(&state.svc, &tab, request)
        .await
        .map(Json)
        .map_err(|error| match error {
            ControlError::Busy => (
                StatusCode::CONFLICT,
                serde_json::json!({"ok":false,"reason":RUNTIME_BUSY_REASON,"error":RUNTIME_BUSY_MESSAGE})
                    .to_string(),
            ),
            ControlError::Refused(reason) => (StatusCode::CONFLICT, reason),
        })
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HostApprovalsRequest {
    allow_all: bool,
}

async fn host_approvals(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(raw_tab): Path<String>,
    Json(request): Json<HostApprovalsRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_permission_authority(&state, &headers)?;
    let tab = crate::routes::resolve_tab_async(&state, &raw_tab)
        .await
        .map_err(|(status, message)| (status, message.to_string()))?;
    let svc = state.svc.clone();
    let target = tab.clone();
    let allow_all =
        tokio::task::spawn_blocking(move || svc.set_tab_host_approvals(&target, request.allow_all))
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "host approvals task join failed".to_string(),
                )
            })?
            .map_err(|reason| (StatusCode::CONFLICT, reason))?;
    Ok(Json(
        serde_json::json!({"tab_id": tab, "allow_all": allow_all}),
    ))
}

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/tabs/{id}/agent/runtime/controls", post(controls))
        .route(
            "/api/tabs/{id}/agent/runtime/host-approvals",
            post(host_approvals),
        )
        .route("/api/tabs/{id}/agent/runtime/goal", post(goal))
        .route("/api/tabs/{id}/agent/runtime/usage", get(tab_usage))
        .route(
            "/api/tabs/{id}/agent/runtime/thread-history",
            post(thread_history),
        )
        .route(
            "/api/tabs/{id}/agent/runtime/reset-credits",
            post(reset_credits),
        )
        .route(
            "/api/tabs/{id}/agent/runtime/login",
            get(show_login).post(login),
        )
        .route(
            "/api/tabs/{id}/agent/runtime/children/{thread}/history",
            get(child_history),
        )
        .route(
            "/api/tabs/{id}/agent/runtime/background-processes",
            get(background_processes).post(stop_background_process),
        )
        .route(
            "/api/tabs/{id}/agent/background-work",
            get(provider_background_output).post(ask_provider_to_stop_background_work),
        )
}

#[cfg(test)]
mod tests;

fn login_response(value: serde_json::Value) -> (HeaderMap, Json<serde_json::Value>) {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    (headers, Json(value))
}
