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

    // recluster is opt-in only: never part of "all", never cron-driven. Runs in-process so the LLM call has direct access to the mem manager.
    if phase == "recluster" {
        return run_recluster_inline(&state).await;
    }

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
        let report = state.svc.mem_supervisor.run(*kind).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("{}: {}", kind.as_arg(), e),
            )
        })?;
        reports.push(report);
    }

    // Run calibration directly in daemon to ensure results are real and history rows are durable.
    if phase == "all" {
        let svc = state.svc.clone();
        let calib = tokio::task::spawn_blocking(move || {
            svc.mem.with_conn(|conn| {
                crate::api::mem_routes::calibration::run_calibration_sweep(conn, false)
            })
        })
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("calibration join: {}", e),
            )
        })?
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("calibration: {}", e),
            )
        })?;
        let mut tagged = calib;
        if let Some(obj) = tagged.as_object_mut() {
            obj.insert(
                "phase".into(),
                serde_json::Value::String("calibration".into()),
            );
        }
        reports.push(tagged);
    }

    // REM worker only fires on a full nightly run; targeted phase calls skip it.
    if phase == "all" {
        let svc = state.svc.clone();
        let mem_cfg = crate::config::memory_config();
        let default_agent = crate::config::default_agent_provider();
        std::thread::spawn(move || dreaming::spawn_rem_worker(&svc, &mem_cfg, &default_agent));
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

async fn run_recluster_inline(
    state: &AppState,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    use crate::mem::dreaming::recluster::{run_recluster_phase_with_lock, AnthropicCaller};

    // Recluster always calls Anthropic directly via `AnthropicCaller` (no
    // tab spawn, no per-provider dispatch) — so unlike every other memory
    let mem_cfg = crate::config::memory_config();
    let (model, _fell_back) = codeterm_mem::worker_agent::resolve_worker_model(
        &mem_cfg.dreaming_model,
        &mem_cfg.worker_model,
        "claude",
        &crate::service::spawn::CatalogBackgroundModelPort,
    );
    let model = if model.is_empty() {
        "sonnet".to_string()
    } else {
        model
    };

    let plan_dir = crate::paths::data_dir().join("mem").join("dream-plans");

    let conn_mutex = state.svc.mem.conn();
    let caller = AnthropicCaller;
    let report = run_recluster_phase_with_lock(conn_mutex, &model, &plan_dir, &caller).await;

    match report {
        Ok(r) => Ok(Json(
            serde_json::json!({ "phase": "recluster", "report": r }),
        )),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}
