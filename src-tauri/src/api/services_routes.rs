use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::service::activity::{ActivityEvent, ActivitySource, ActivityWriter};
use crate::service::store::{self, Service, ServiceCreate, ServiceKind, ServiceUpdate};

#[derive(Deserialize)]
pub struct ActivityQuery {
    pub source: Option<String>,
    pub since: Option<i64>,
    pub limit: Option<usize>,
}

pub async fn list_activity(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<ActivityQuery>,
) -> Result<Json<Vec<ActivityEvent>>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let writer = ActivityWriter::new(state.svc.services_store.conn());
    let source = q.source.as_deref().and_then(parse_activity_source);
    writer
        .list(source, q.since, q.limit)
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

fn parse_activity_source(s: &str) -> Option<ActivitySource> {
    match s {
        "push" => Some(ActivitySource::Push),
        "health" => Some(ActivitySource::Health),
        "peer" => Some(ActivitySource::Peer),
        "cron" => Some(ActivitySource::Cron),
        _ => None,
    }
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub kind: Option<String>,
    pub enabled: Option<bool>,
}

fn parse_kind(s: &str) -> Option<ServiceKind> {
    match s {
        "cron" => Some(ServiceKind::Cron),
        "daemon" => Some(ServiceKind::Daemon),
        _ => None,
    }
}

pub async fn list_services(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<Service>>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let kind = q.kind.as_deref().and_then(parse_kind);
    let merged = {
        let cron = state.cron.lock();
        store::list_local(&state.svc.services_store, &cron)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };
    let filtered: Vec<Service> = merged
        .into_iter()
        .filter(|s| kind.as_ref().map_or(true, |k| &s.kind == k))
        .filter(|s| q.enabled.map_or(true, |e| s.enabled == e))
        .collect();
    Ok(Json(filtered))
}

pub async fn create_service(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<ServiceCreate>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    if body.kind != ServiceKind::Daemon {
        return Err((
            StatusCode::BAD_REQUEST,
            "Only daemon-kind services can be created via this endpoint; use /api/crons for cron jobs"
                .into(),
        ));
    }
    let id = state
        .svc
        .services_store
        .create(body)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let token = state
        .svc
        .tokens
        .generate(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let service = state
        .svc
        .services_store
        .get(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((
            StatusCode::INTERNAL_SERVER_ERROR,
            "service vanished after create".into(),
        ))?;
    Ok(Json(
        serde_json::json!({"service": service, "token": token}),
    ))
}

pub async fn get_service(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Service>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    state
        .svc
        .services_store
        .get(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, format!("service {} not found", id)))
}

pub async fn update_service(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ServiceUpdate>,
) -> Result<Json<Service>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    state
        .svc
        .services_store
        .update(&id, body)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state
        .svc
        .services_store
        .get(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .ok_or((
            StatusCode::NOT_FOUND,
            format!("service {} not found after update", id),
        ))
}

pub async fn delete_service(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    state
        .svc
        .services_store
        .delete(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if let Err(e) = state.svc.tokens.delete(&id) {
        log::warn!(
            "[services] deleted service {} but failed to delete push token: {}",
            id,
            e
        );
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn regenerate_token(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let token = state
        .svc
        .tokens
        .generate(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if let Err(e) = state.svc.services_store.update(
        &id,
        ServiceUpdate {
            push_enabled: Some(true),
            ..Default::default()
        },
    ) {
        log::warn!(
            "[services] regenerated token for {} but failed to mark push enabled: {}",
            id,
            e
        );
    }
    Ok(Json(
        serde_json::json!({"token": token, "push_enabled": true}),
    ))
}
