use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

use super::routes::AppState;

type ApiError = (StatusCode, String);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/agents/providers/{id}/model-usage", get(model_usage))
        .route("/api/system/default-cwd", get(default_cwd))
        .route("/api/system/shell-diagnostics", get(shell_diagnostics))
        .route("/api/system/install-pwsh", post(install_pwsh))
        .route("/api/themes/location", get(themes_location))
        .route("/api/attachments/path-kinds", post(path_kinds))
        .route("/api/diagnostics/webgl", post(webgl))
}

fn require_owner(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    if crate::auth::is_local_caller(state, headers) {
        Ok(())
    } else {
        Err((StatusCode::FORBIDDEN, "Owner authorization required".into()))
    }
}

async fn model_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Vec<codeterm_service_core::agent::provider::ModelGroupUsage>>, ApiError> {
    require_owner(&state, &headers)?;
    let Some(provider) = state.svc.agent_registry.provider_arc(&id) else {
        return Ok(Json(Vec::new()));
    };
    tokio::task::spawn_blocking(move || provider.model_group_usage())
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn default_cwd(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<String>, ApiError> {
    require_owner(&state, &headers)?;
    codeterm_commands::commands::get_default_cwd()
        .map(Json)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))
}

async fn shell_diagnostics(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_owner(&state, &headers)?;
    Ok(Json(
        codeterm_commands::commands::get_shell_diagnostics().await,
    ))
}

async fn install_pwsh(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<String>, ApiError> {
    require_owner(&state, &headers)?;
    if !pwsh_install_supported(std::env::consts::OS) {
        return Err((
            StatusCode::NOT_IMPLEMENTED,
            "PowerShell installation requires a Windows host".into(),
        ));
    }
    let service = state.svc.clone();
    tokio::task::spawn_blocking(move || {
        service
            .spawn_tab(codeterm_service_core::service::spawn::SpawnTabSpec {
                shell: Some("powershell".into()),
                title: Some("InstallPwsh".into()),
                cmd: Some("winget install Microsoft.PowerShell".into()),
                ..Default::default()
            })
            .map(|handle| Json(handle.tab_id))
    })
    .await
    .map_err(internal_error)?
    .map_err(|error| (StatusCode::BAD_REQUEST, error))
}

fn pwsh_install_supported(platform: &str) -> bool {
    platform == "windows"
}

async fn themes_location(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<String>, ApiError> {
    require_owner(&state, &headers)?;
    codeterm_service_core::config::schema::user_themes_dir()
        .map(|path| Json(path.to_string_lossy().into_owned()))
        .ok_or((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Cannot determine themes directory".into(),
        ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathKindsBody {
    paths: Vec<String>,
}

fn validate_paths(paths: &[String]) -> Result<(), ApiError> {
    if paths.len() > 128
        || paths
            .iter()
            .any(|path| path.len() > 32768 || path.contains('\0'))
    {
        return Err((StatusCode::BAD_REQUEST, "Invalid path batch".into()));
    }
    Ok(())
}

async fn path_kinds(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PathKindsBody>,
) -> Result<Json<Vec<String>>, ApiError> {
    require_owner(&state, &headers)?;
    validate_paths(&body.paths)?;
    tokio::task::spawn_blocking(move || {
        body.paths
            .iter()
            .map(|path| {
                codeterm_service_core::attachments::dropped_path_kind(std::path::Path::new(path))
                    .to_owned()
            })
            .collect()
    })
    .await
    .map(Json)
    .map_err(internal_error)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WebglBody {
    state: String,
    detail: Option<String>,
}

async fn webgl(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<WebglBody>,
) -> Result<Json<()>, ApiError> {
    require_owner(&state, &headers)?;
    validate_webgl(&body)?;
    codeterm_commands::commands::report_webgl_status(body.state, body.detail);
    Ok(Json(()))
}

fn validate_webgl(body: &WebglBody) -> Result<(), ApiError> {
    if !matches!(
        body.state.as_str(),
        "loaded"
            | "context_lost"
            | "failed"
            | "skipped_macos"
            | "skipped_windows"
            | "skipped_test_mode"
    ) || body
        .detail
        .as_ref()
        .is_some_and(|detail| detail.len() > 4096)
    {
        return Err((StatusCode::BAD_REQUEST, "Invalid WebGL diagnostic".into()));
    }
    Ok(())
}

fn internal_error(error: tokio::task::JoinError) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

#[cfg(test)]
mod tests;
