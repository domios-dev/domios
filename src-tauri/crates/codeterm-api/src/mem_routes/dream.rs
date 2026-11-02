use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::routes::{check_auth, AppState};
use codeterm_service_core::mem::dreaming;
use codeterm_service_core::mem_ops::dream::*;

pub async fn mem_extract_pending(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<ExtractPendingParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let limit = p.limit.unwrap_or(200).clamp(1, 1000);
    let count =
        codeterm_service_core::mem::auto_extract::run_extract_pending(state.svc.clone(), limit);
    Ok(Json(serde_json::json!({ "queued": count })))
}

pub async fn mem_dream(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<DreamParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    use codeterm_service_core::mem::supervisor::WorkerKind;

    let phase = p.phase.as_deref().unwrap_or("all").to_lowercase();

    // recluster is opt-in only: never part of "all", never cron-driven. Runs in-process so the LLM call has direct access to the mem manager.
    if phase == "recluster" {
        let result = run_recluster_inline(&state).await;
        if result.is_ok() {
            state
                .svc
                .record_product_feature(codeterm_analytics::Feature::DreamingRun);
        }
        return result;
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
        let report = state
            .svc
            .ext::<std::sync::Arc<codeterm_service_core::mem::supervisor::Supervisor>>()
            .run(*kind)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("{}: {}", kind.as_arg(), e),
                )
            })?;
        state
            .svc
            .record_product_feature(codeterm_analytics::Feature::DreamingRun);
        reports.push(report);
    }

    // Run calibration directly in daemon to ensure results are real and history rows are durable.
    if phase == "all" {
        let svc = state.svc.clone();
        let hygiene = state.svc.configuration().memory.dream_hygiene;
        let calib = tokio::task::spawn_blocking(move || {
            svc.mem.nightly_calibration(&hygiene, chrono::Utc::now())
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
        let mem_cfg = state.svc.configuration().memory;
        let default_agent = state.svc.configuration().general_agent.agent_provider;
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
    use codeterm_service_core::mem::dreaming::recluster::{
        run_recluster_phase_with_store, AnthropicCaller,
    };

    // Recluster uses the fixed Claude model catalog rather than an agent-provider ladder.
    let mem_cfg = state.svc.configuration().memory;
    let (model, _fell_back) = codeterm_mem::worker_agent::resolve_worker_model(
        &mem_cfg.dreaming_model,
        &mem_cfg.worker_model,
        "claude",
        &codeterm_service_core::service::spawn::CatalogBackgroundModelPort,
    );
    let model = if model.is_empty() {
        "sonnet".to_string()
    } else {
        model
    };

    let plan_dir = codeterm_service_core::paths::data_dir()
        .join("mem")
        .join("dream-plans");

    let caller = AnthropicCaller;
    let report = run_recluster_phase_with_store(&state.svc.mem, &model, &plan_dir, &caller).await;

    match report {
        Ok(r) => Ok(Json(
            serde_json::json!({ "phase": "recluster", "report": r }),
        )),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

pub async fn mem_dream_diary(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<DreamDiaryParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let offset = p.offset.unwrap_or(0);
    let limit = p.limit.unwrap_or(30).clamp(1, 100);
    let svc = state.svc.clone();
    tokio::task::spawn_blocking(move || svc.mem.diary_page(offset, limit))
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("diary join: {e}"),
            )
        })?
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

#[cfg(test)]
use codeterm_mem::backend::sqlite::diary_page;

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rusqlite::Connection;

    use super::diary_page;

    #[test]
    fn dream_diary_route_orders_scope_before_limit_and_reports_total() {
        let conn = Connection::open_in_memory().unwrap();
        codeterm_service_core::mem::store::init_schema(&conn).unwrap();

        for day in 1..=32 {
            let id = format!("daily_dream_relevant_{day:02}");
            let created_at = format!("2026-07-{day:02}T06:00:00.000000+00:00");
            conn.execute(
                "INSERT INTO mem_items
                 (id, uid, kind, scope, title, area, tags_json, body_inline, importance, created_at, updated_at)
                 VALUES (?1, ?1, 'daily', 'global', ?2, '_dreams', '[\"dream-diary\"]', ?3, 5.0, ?4, ?4)",
                rusqlite::params![id, format!("dream: {created_at}"), "older diary body", created_at],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO mem_items
             (id, uid, kind, scope, title, area, tags_json, body_inline, importance, created_at, updated_at)
             VALUES ('daily_dream_fresh', 'daily_dream_fresh', 'daily', 'global',
                     'dream: fresh', '_dreams', '[\"dream-diary\"]', 'fresh diary body', 0.1,
                     '2026-08-05T06:00:00.000000+00:00', '2026-08-05T06:00:00.000000+00:00')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO mem_items
             (id, uid, kind, scope, title, area, tags_json, body_inline, importance, created_at, updated_at)
             VALUES ('not_a_dream', 'not_a_dream', 'daily', 'global', 'not a dream', 'work', '[]',
                     'unrelated body', 5.0, '2026-08-06T06:00:00.000000+00:00',
                     '2026-08-06T06:00:00.000000+00:00')",
            [],
        )
        .unwrap();

        let page = diary_page(&conn, Path::new("."), 0, 1).unwrap();
        assert_eq!(page["total"], 33);
        assert_eq!(page["items"][0]["id"], "daily_dream_fresh");
        assert_eq!(page["next_offset"], 1);

        let second_page = diary_page(&conn, Path::new("."), 1, 1).unwrap();
        assert_eq!(second_page["items"][0]["id"], "daily_dream_relevant_32");
        assert_eq!(second_page["next_offset"], 2);
    }
}
