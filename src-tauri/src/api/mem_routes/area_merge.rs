use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use super::{emit_activity, WORKER_ID_HEADER};
use crate::{
    api::routes::{check_auth, AppState},
    mem::store,
    service::ServiceState,
};

#[derive(Deserialize)]
pub struct AreaMergeBody {
    pub from: String,
    pub into: String,
    #[serde(default)]
    pub dry_run: bool,
    pub worker_id: Option<String>,
}

pub(crate) fn merge_area(
    svc: &ServiceState,
    body: &AreaMergeBody,
    worker_id: Option<&str>,
) -> Result<serde_json::Value, String> {
    let intent = worker_id.and_then(|id| svc.workers.get(id).map(|worker| worker.intent));
    let dreaming = intent
        .as_deref()
        .is_some_and(crate::mem::workers::is_dreaming_intent);
    let actor = super::mem_merge::merge_actor(intent.as_deref(), None);
    let outcome = store::with_changed_by(&actor, || {
        svc.mem.with_conn(|conn| {
            store::area_merge::merge_area(
                conn,
                svc.mem.blobs_dir(),
                &body.from,
                &body.into,
                body.dry_run,
                if dreaming { worker_id } else { None },
            )
        })
    })?;
    if !body.dry_run {
        for id in &outcome.item_ids {
            svc.mem.refresh_embedding(id);
        }
    }
    serde_json::to_value(outcome).map_err(|e| e.to_string())
}

pub async fn mem_area_merge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AreaMergeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let worker = headers
        .get(WORKER_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or_else(|| body.worker_id.clone());
    let svc = state.svc.clone();
    let dry_run = body.dry_run;
    let output = tokio::task::spawn_blocking(move || merge_area(&svc, &body, worker.as_deref()))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    if !dry_run {
        let ids = serde_json::from_value(output["item_ids"].clone()).unwrap_or_default();
        emit_activity(&state, &headers, "update", ids);
    }
    Ok(Json(output))
}
const _SPLIT_METRIC_PROBE: u8 = 0;
