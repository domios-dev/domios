use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::routes::{check_auth, AppState};

pub async fn list_themes_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let ids = codeterm_service_core::config::schema::list_all_theme_ids();
    Ok(Json(serde_json::json!({ "themes": ids })))
}

pub(crate) fn valid_theme_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[derive(Deserialize)]
pub(crate) struct SaveThemeBody {
    id: String,
    toml: String,
}

pub async fn save_theme_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<SaveThemeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    if !valid_theme_id(&body.id) {
        return Err((StatusCode::BAD_REQUEST, "Invalid theme identity"));
    }
    let dir = codeterm_service_core::config::schema::user_themes_dir().ok_or((
        StatusCode::INTERNAL_SERVER_ERROR,
        "Cannot determine themes directory",
    ))?;
    std::fs::create_dir_all(&dir).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create themes dir",
        )
    })?;
    codeterm_service_core::config::schema::parse_builtin_theme(&body.toml)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid theme TOML"))?;
    let path = dir.join(format!("{}.toml", body.id));
    std::fs::write(&path, &body.toml).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to write theme file",
        )
    })?;
    Ok(Json(
        serde_json::json!({ "ok": true, "path": path.to_string_lossy() }),
    ))
}

pub async fn delete_theme_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    if !valid_theme_id(&id) {
        return Err((StatusCode::BAD_REQUEST, "Invalid theme identity"));
    }
    if let Some(dir) = codeterm_service_core::config::schema::user_themes_dir() {
        let path = dir.join(format!("{}.toml", id));
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct ApplyThemeBody {
    name: String,
}

pub async fn apply_theme_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<ApplyThemeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    use tauri::Manager;

    let config_state = state
        .svc
        .app_handle
        .state::<codeterm_service_core::config::ConfigState>();
    let mut config = config_state.lock().clone();
    config.theme.name = body.name.clone();
    if let Some(mut theme) = codeterm_service_core::config::schema::get_any_theme(&body.name) {
        theme.name = body.name;
        config.theme = theme;
    }
    codeterm_service_core::config::save_config_pub(&config)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    *config_state.lock() = config.clone();
    crate::events::emit_everywhere(&state.svc.app_handle, "config-changed", &config);
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Query string: `?provider=<id>` (default `claude` for back-compat —
/// the old endpoints had no provider param and were Claude-only).
#[derive(serde::Deserialize)]
pub struct ProviderQuery {
    #[serde(default = "default_provider")]
    pub provider: String,
}

fn default_provider() -> String {
    "claude".into()
}

pub async fn check_rules_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<ProviderQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    if !crate::auth::is_local_caller(&state, &headers) {
        return Err((StatusCode::FORBIDDEN, "Owner authorization required"));
    }
    let provider = state
        .svc
        .agent_registry
        .provider_arc(&q.provider)
        .ok_or((StatusCode::NOT_FOUND, "Unknown provider"))?;
    let status = tokio::task::spawn_blocking(move || provider.check_rule_status())
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Rule status task failed"))?;
    Ok(Json(status))
}

pub async fn install_rules_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<ProviderQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    if !crate::auth::is_local_caller(&state, &headers) {
        return Err((StatusCode::FORBIDDEN, "Owner authorization required"));
    }
    let provider = state
        .svc
        .agent_registry
        .provider_arc(&q.provider)
        .ok_or((StatusCode::NOT_FOUND, "Unknown provider"))?;
    tokio::task::spawn_blocking(move || provider.install_rule())
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Rule install task failed",
            )
        })?
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to install rule"))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn restore_hooks_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<ProviderQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if !crate::auth::is_local_caller(&state, &headers) {
        return Err((StatusCode::FORBIDDEN, "Owner authorization required".into()));
    }
    if state.svc.agent_registry.provider_arc(&q.provider).is_none() {
        return Err((StatusCode::NOT_FOUND, "Unknown provider".into()));
    }
    let registry = state.svc.agent_registry.clone();
    tokio::task::spawn_blocking(move || {
        codeterm_service_core::service::provider_hooks::restore_provider_integration_core(
            &registry,
            &q.provider,
        )
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| (StatusCode::CONFLICT, error))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn check_hooks_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<ProviderQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let (status, has_hooks) = state
        .svc
        .agent_registry
        .with_provider(&q.provider, |provider| {
            (provider.check_integration(), provider.has_hooks())
        })
        .ok_or((StatusCode::NOT_FOUND, "Unknown provider"))?;
    let live_session_ids = state
        .svc
        .mux
        .read()
        .all_tabs()
        .filter(|tab| tab.agent_type.as_deref() == Some(q.provider.as_str()))
        .filter_map(|tab| tab.agent_session_id.clone())
        .collect::<Vec<_>>();
    Ok(Json(
        codeterm_service_core::service::integration_silence::integration_status_with_binding_fallback(
            status,
            &q.provider,
            has_hooks,
            &live_session_ids,
        ),
    ))
}

#[derive(Deserialize)]
pub struct SweepProfilesQuery {
    pub provider: String,
    #[serde(default)]
    pub confirm: bool,
    #[serde(default)]
    pub include_legacy: bool,
}

pub async fn sweep_profiles_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<SweepProfilesQuery>,
) -> Result<
    Json<codeterm_service_core::agent::provider::LaunchArtifactSweepReport>,
    (StatusCode, String),
> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    let service = state.svc.clone();
    let report = tokio::task::spawn_blocking(move || {
        codeterm_service_core::service::launch_artifacts::sweep_for_owner(
            &service,
            &query.provider,
            query.confirm,
            query.include_legacy,
        )
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    Ok(Json(report))
}

#[derive(serde::Deserialize)]
pub struct InstallHooksQuery {
    pub provider: String,
}

/// An authenticated install request is the user's consent: it is recorded like
/// the Settings install, and the install grants trust to CodeTerm's own hooks.
pub async fn install_hooks_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<InstallHooksQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if !crate::auth::is_local_caller(&state, &headers) {
        return Err((StatusCode::FORBIDDEN, "Owner authorization required".into()));
    }
    if state
        .svc
        .agent_registry
        .with_provider(&q.provider, |_| ())
        .is_none()
    {
        return Err((StatusCode::NOT_FOUND, "Unknown provider".to_string()));
    }
    let registry = state.svc.agent_registry.clone();
    let config = state.config.clone();
    let api_port = state.api_port;
    let provider_id = q.provider.clone();
    tokio::task::spawn_blocking(move || {
        codeterm_service_core::service::provider_hooks::install_provider_integration_with_consent(
            &registry,
            &config,
            provider_id,
            api_port,
        )
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let (status, has_hooks) = state
        .svc
        .agent_registry
        .with_provider(&q.provider, |provider| {
            (provider.check_integration(), provider.has_hooks())
        })
        .unwrap_or((serde_json::Value::Null, false));
    let live_session_ids = state
        .svc
        .mux
        .read()
        .all_tabs()
        .filter(|tab| tab.agent_type.as_deref() == Some(q.provider.as_str()))
        .filter_map(|tab| tab.agent_session_id.clone())
        .collect::<Vec<_>>();
    let status = codeterm_service_core::service::integration_silence::integration_status_with_binding_fallback(
        status,
        &q.provider,
        has_hooks,
        &live_session_ids,
    );
    Ok(Json(serde_json::json!({
        "ok": true,
        "provider": q.provider,
        "status": status,
    })))
}

pub async fn uninstall_hooks_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<ProviderQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if !crate::auth::is_local_caller(&state, &headers) {
        return Err((StatusCode::FORBIDDEN, "Owner authorization required".into()));
    }
    if state.svc.agent_registry.provider_arc(&q.provider).is_none() {
        return Err((StatusCode::NOT_FOUND, "Unknown provider".into()));
    }
    let registry = state.svc.agent_registry.clone();
    tokio::task::spawn_blocking(move || {
        codeterm_service_core::service::provider_hooks::uninstall_provider_integration_core(
            &registry,
            &q.provider,
        )
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| (StatusCode::CONFLICT, error))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[cfg(test)]
pub(crate) mod tests;
