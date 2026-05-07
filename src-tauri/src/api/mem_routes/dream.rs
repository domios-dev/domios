use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::mem::dreaming;

#[derive(Deserialize)]
pub struct ExtractPendingParams {
    #[serde(default)]
    pub limit: Option<usize>,
}

pub async fn mem_extract_pending(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<ExtractPendingParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let limit = p.limit.unwrap_or(200).clamp(1, 1000);
    let count = crate::mem::auto_extract::run_extract_pending(state.svc.clone(), limit);
    Ok(Json(serde_json::json!({ "queued": count })))
}

#[derive(Deserialize, Default)]
pub struct DreamParams {
    #[serde(default)]
    pub phase: Option<String>,
}

pub async fn mem_dream(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<DreamParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    use crate::mem::supervisor::WorkerKind;

    let phase = p.phase.as_deref().unwrap_or("all").to_lowercase();
    let phases: Vec<WorkerKind> = match phase.as_str() {
        "light" => vec![WorkerKind::DreamingLight],
        "deep" => vec![WorkerKind::DreamingDeep],
        "normalize" => vec![WorkerKind::DreamingNormalize],
        "all" => vec![
            WorkerKind::DreamingLight,
            WorkerKind::DreamingDeep,
            WorkerKind::DreamingNormalize,
        ],
        other => return Err((StatusCode::BAD_REQUEST, format!("unknown phase: {}", other))),
    };

    let mut reports = Vec::new();
    for kind in &phases {
        let report = state.svc.mem_supervisor.run(*kind).await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{}: {}", kind.as_arg(), e)))?;
        reports.push(report);
    }

    // REM worker only fires on a full nightly run; targeted phase calls skip it.
    if phase == "all" {
        let svc = state.svc.clone();
        std::thread::spawn(move || dreaming::spawn_rem_worker(&svc));
    }

    Ok(Json(serde_json::json!({ "phases": reports })))
}

pub async fn mem_dream_report(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let report = dreaming::prepare_rem_data(&state.svc.mem)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "report": report })))
}
