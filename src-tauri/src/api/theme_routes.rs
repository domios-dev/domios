use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};

pub async fn list_themes_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let ids = crate::config::schema::list_all_theme_ids();
    Ok(Json(serde_json::json!({ "themes": ids })))
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
    let dir = crate::config::schema::user_themes_dir()
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "Cannot determine themes directory"))?;
    std::fs::create_dir_all(&dir).map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create themes dir"))?;
    crate::config::schema::parse_builtin_theme(&body.toml)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid theme TOML"))?;
    let path = dir.join(format!("{}.toml", body.id));
    std::fs::write(&path, &body.toml)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to write theme file"))?;
    Ok(Json(serde_json::json!({ "ok": true, "path": path.to_string_lossy() })))
}

pub async fn delete_theme_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    if let Some(dir) = crate::config::schema::user_themes_dir() {
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
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    use tauri::{Emitter, Manager};

    let config_state = state.svc.app_handle.state::<crate::config::ConfigState>();
    let mut config = config_state.lock().clone();
    config.theme.name = body.name.clone();
    if let Some(mut theme) = crate::config::schema::get_any_theme(&body.name) {
        theme.name = body.name;
        config.theme = theme;
    }
    crate::config::save_config_pub(&config)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to save config"))?;
    *config_state.lock() = config.clone();
    let _ = state.svc.app_handle.emit("config-changed", &config);
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
    check_auth(&state, &headers)?;
    let status = state.svc.agent_registry
        .with_provider(&q.provider, |p| p.check_rule_status())
        .ok_or((StatusCode::NOT_FOUND, "Unknown provider"))?;
    Ok(Json(status))
}

pub async fn install_rules_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<ProviderQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state.svc.agent_registry
        .with_provider(&q.provider, |p| p.install_rule())
        .ok_or((StatusCode::NOT_FOUND, "Unknown provider"))?
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to install rule"))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn check_hooks_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<ProviderQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let status = state.svc.agent_registry
        .with_provider(&q.provider, |p| p.check_integration())
        .ok_or((StatusCode::NOT_FOUND, "Unknown provider"))?;
    Ok(Json(status))
}

pub async fn install_hooks_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<ProviderQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let port = state.api_port;
    state.svc.agent_registry
        .with_provider(&q.provider, |p| p.install_integration(port))
        .ok_or((StatusCode::NOT_FOUND, "Unknown provider"))?
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to install integration"))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn uninstall_hooks_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<ProviderQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    state.svc.agent_registry
        .with_provider(&q.provider, |p| p.uninstall_integration())
        .ok_or((StatusCode::NOT_FOUND, "Unknown provider"))?
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to uninstall integration"))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
