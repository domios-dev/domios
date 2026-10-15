mod wake;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{auth::CallerIdentity, routes::AppState};

#[derive(Deserialize)]
pub(crate) struct CwdBody {
    cwd: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/tabs/{id}/wake", post(wake::wake))
        .route("/api/tabs/{id}/cwd", post(update_cwd))
        .route("/api/tabs/{id}/history", get(history))
}

fn local_target(caller: &CallerIdentity, raw: &str) -> Result<(), (StatusCode, &'static str)> {
    if !matches!(caller, CallerIdentity::Local) {
        return Err((StatusCode::FORBIDDEN, "Local owner required"));
    }
    if raw.starts_with("mesh:") {
        return Err((StatusCode::BAD_REQUEST, "Remote tab requires peer relay"));
    }
    Ok(())
}

async fn update_cwd(
    State(state): State<AppState>,
    Extension(caller): Extension<CallerIdentity>,
    Path(raw): Path<String>,
    Json(body): Json<CwdBody>,
) -> Result<Json<Value>, (StatusCode, &'static str)> {
    local_target(&caller, &raw)?;
    let tab_id = crate::routes::resolve_tab(&state, &raw)?;
    state.svc.update_tab_cwd(&tab_id, body.cwd);
    Ok(Json(json!({ "ok": true })))
}

async fn history(
    State(state): State<AppState>,
    Extension(caller): Extension<CallerIdentity>,
    Path(raw): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    local_target(&caller, &raw).map_err(|(status, message)| (status, message.into()))?;
    let tab_id = crate::routes::resolve_tab(&state, &raw)
        .map_err(|(status, message)| (status, message.into()))?;
    let command = codeterm_service_core::dispatch::Command::GetOutput {
        tab_id,
        offset: None,
        max_bytes: None,
        strip_ansi: false,
        full: true,
    };
    let result = codeterm_service_core::dispatch::dispatch_blocking(command, state.svc.clone())
        .await
        .map_err(|reason| (StatusCode::NOT_FOUND, reason))?;
    Ok(Json(result.into_json()))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod fixture;
