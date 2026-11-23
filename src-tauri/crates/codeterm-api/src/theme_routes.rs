use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::routes::AppState;

use codeterm_service_core::config::schema::{AppearanceLook, MAX_THEME_BYTES};

pub async fn list_themes_api(
    AxumState(_state): AxumState<AppState>,
    _headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    tokio::task::spawn_blocking(move || {
        let ids = codeterm_service_core::config::schema::list_all_theme_ids();
        Ok(Json(serde_json::json!({ "themes": ids })))
    })
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Theme operation failed"))?
}

pub(crate) fn valid_theme_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[derive(Deserialize)]
pub struct SaveThemeBody {
    id: String,
    toml: String,
}

pub async fn save_theme_api(
    AxumState(_state): AxumState<AppState>,
    _headers: HeaderMap,
    Json(body): Json<SaveThemeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    tokio::task::spawn_blocking(move || {
        if !valid_theme_id(&body.id) {
            return Err(theme_save_error(
                StatusCode::BAD_REQUEST,
                "invalid_theme_id",
                "Invalid theme identity",
            ));
        }
        if body.toml.len() > MAX_THEME_BYTES {
            return Err(theme_save_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "theme_too_large",
                "Theme exceeds the size limit",
            ));
        }
        let theme = codeterm_service_core::config::schema::parse_builtin_theme(&body.toml)
            .map_err(|error| {
                let message = error.message();
                if message.starts_with("chat_background") {
                    theme_save_error(StatusCode::BAD_REQUEST, "invalid_chat_background", message)
                } else {
                    theme_save_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_theme_toml",
                        "Invalid theme TOML",
                    )
                }
            })?;
        crate::appearance_routes::validate(&AppearanceLook {
            theme,
            wallpaper: None,
            tuning: None,
        })
        .map_err(crate::appearance_routes::typed_refusal)?;
        let dir = codeterm_service_core::config::schema::user_themes_dir().ok_or_else(|| {
            theme_save_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "theme_storage_failed",
                "Cannot determine themes directory",
            )
        })?;
        let path = dir.join(format!("{}.toml", body.id));
        codeterm_util::atomic_write::write_atomic(&path, body.toml.as_bytes()).map_err(|_| {
            theme_save_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "theme_storage_failed",
                "Failed to write theme file",
            )
        })?;
        Ok(Json(
            serde_json::json!({ "ok": true, "path": path.to_string_lossy() }),
        ))
    })
    .await
    .map_err(|_| {
        theme_save_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "theme_storage_failed",
            "Theme operation failed",
        )
    })?
}

fn theme_save_error(
    status: StatusCode,
    code: &'static str,
    message: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({ "code": code, "message": message })),
    )
}

pub async fn delete_theme_api(
    AxumState(_state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    tokio::task::spawn_blocking(move || {
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
    })
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Theme operation failed"))?
}

#[derive(Deserialize)]
pub struct ApplyThemeBody {
    name: String,
}

pub async fn apply_theme_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<ApplyThemeBody>,
) -> Result<Json<serde_json::Value>, crate::appearance_routes::ApiRefusal> {
    let agent = crate::auth::off_runtime(&state, &headers, |state, headers| {
        Ok(crate::auth::authenticated_origin(state, headers)
            .is_ok_and(|origin| origin == crate::auth::AuthenticatedOrigin::Agent))
    })
    .await
    .map_err(crate::appearance_routes::typed_refusal)?;
    apply_theme_with(state, body, agent, commit_theme)
        .await
        .map_err(crate::appearance_routes::typed_refusal)
}

pub(super) async fn apply_theme_with(
    state: AppState,
    body: ApplyThemeBody,
    agent: bool,
    save: impl FnOnce(
            &codeterm_service_core::config::ConfigState,
            &str,
        ) -> Result<
            (codeterm_service_core::config::schema::AppConfig, bool),
            (StatusCode, String),
        > + Send
        + 'static,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if !valid_theme_id(&body.name) {
        return Err((StatusCode::BAD_REQUEST, "Invalid theme identity".into()));
    }
    tokio::task::spawn_blocking(move || {
        let (config, changed) = save(&state.config, &body.name)?;
        if agent && changed {
            crate::appearance_routes::emit_change(&state, &config, None, false);
        }
        state.svc.ui.emit(
            "config-changed",
            super::config_routes::redact_rest_config(
                serde_json::to_value(&config).unwrap_or_default(),
            ),
        );
        Ok(Json(serde_json::json!({ "ok": true })))
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

fn commit_theme(
    state: &codeterm_service_core::config::ConfigState,
    name: &str,
) -> Result<(codeterm_service_core::config::schema::AppConfig, bool), (StatusCode, String)> {
    let mut theme = codeterm_service_core::config::schema::get_any_theme(name)
        .ok_or((StatusCode::NOT_FOUND, "Theme is unavailable".into()))?;
    theme.name = name.to_owned();
    let mut validation_failure = None;
    let mut changed = false;
    let config = codeterm_service_core::config::update_config_and_apply(state, |config| {
        changed = config.theme != theme;
        let mut look = AppearanceLook::from_config(config);
        look.theme = theme;
        crate::appearance_routes::validate(&look).map_err(|(status, message)| {
            validation_failure = Some((status, message.clone()));
            message
        })?;
        look.write_to(config);
        if changed {
            config.appearance_active = None;
        }
        Ok(())
    })
    .map_err(|error| validation_failure.unwrap_or((StatusCode::INTERNAL_SERVER_ERROR, error)))?;
    Ok((config, changed))
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
    _headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<ProviderQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    tokio::task::spawn_blocking(move || {
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
    })
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "blocking task failed"))?
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
    _headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<SweepProfilesQuery>,
) -> Result<
    Json<codeterm_service_core::agent::provider::LaunchArtifactSweepReport>,
    (StatusCode, String),
> {
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
    tokio::task::spawn_blocking(move || {
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

}).await.map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "blocking task failed".to_string()))?
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
    let svc = state.svc.clone();
    tokio::task::spawn_blocking(move || {
        codeterm_service_core::service::provider_hooks::uninstall_provider_integration_core(
            &svc,
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
