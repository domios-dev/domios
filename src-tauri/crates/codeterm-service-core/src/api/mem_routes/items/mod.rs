use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::api::routes::{check_auth, AppState};
use crate::mem::activity::now_ms;
use crate::mem::{
    learnings::{self, LearnKind},
    store::{MemKind, MemScope, MemUpdate, NewMemItem},
};

use super::{changed_by_from_headers, emit_activity, sanitize_actor, WORKER_ID_HEADER};
use crate::mem_ops::items::*;
use crate::mem_ops::validate_item_fields;

pub async fn mem_activity_recent(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<ActivityRecentParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let since = p
        .since_ms
        .unwrap_or_else(|| now_ms().saturating_sub(60_000));
    let events = state.svc.activity.recent(since);
    Ok(Json(serde_json::json!({ "events": events })))
}
pub async fn mem_get(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let item = state
        .svc
        .mem
        .get(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    match item {
        Some(item) => {
            emit_activity(&state, &headers, "read", vec![id.clone()]);
            // Skip evidence bump on worker-driven reads (extraction/dreamer sweeps would poison the signal).
            let is_worker_call = headers.get(WORKER_ID_HEADER).is_some();
            if !is_worker_call {
                state
                    .svc
                    .mem
                    .with_conn(|conn| bump_on_get(conn, &item.kind, &id));
            }
            Ok(Json(serde_json::to_value(item).unwrap_or_default()))
        }
        None => Err((StatusCode::NOT_FOUND, format!("mem item {} not found", id))),
    }
}

pub async fn mem_pinned_list(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<PinnedListParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let limit = p.limit.unwrap_or(100).clamp(1, 500);
    let items = state
        .svc
        .mem
        .list_pinned(limit)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "items": items })))
}

mod archive;
mod delete;
pub use archive::mem_archive;
pub use delete::{mem_delete, mem_forget_area};

pub async fn mem_save(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<SaveBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    validate_item_fields(
        &body.title,
        &body.body,
        &body.area,
        &body.tags,
        body.importance,
    )?;
    let kind = MemKind::from_str(&body.kind).ok_or((
        StatusCode::BAD_REQUEST,
        format!("unknown kind: {}", body.kind),
    ))?;
    let scope = body
        .scope
        .as_deref()
        .map(MemScope::from_str)
        .transpose()
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?
        .unwrap_or(MemScope::Global);
    let is_auto_extracted = body.tags.iter().any(|t| t == "auto-extracted")
        || body
            .source
            .as_deref()
            .map_or(false, |s| s.starts_with("extracted_from:"));

    let item = NewMemItem {
        kind,
        scope,
        title: body.title,
        summary: body.summary,
        area: body.area,
        tags: body.tags,
        body: body.body,
        source: body.source,
        importance: body.importance.unwrap_or(3.0),
        source_path: body.source_path,
        pinned: body.pinned.unwrap_or(false),
    };
    let title_clone = item.title.clone();
    let body_clone = item.body.clone();
    let kind_for_extract = item.kind;
    let changed_by = changed_by_from_headers(&headers);
    let worker_id = super::worker_id_from_headers(&headers);
    let result = crate::mem::store::with_write_worker(worker_id, || {
        crate::mem::store::with_changed_by(&changed_by, || {
            state.svc.mem.save_with_outcome(item, body.replace)
        })
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !is_auto_extracted {
        state
            .svc
            .record_product_feature(codeterm_analytics::Feature::MemorySaved);
    }
    let id = result.id;
    let outcome = result.outcome;
    emit_activity(&state, &headers, "write", vec![id.clone()]);

    let auto_extract_enabled = state.svc.configuration().memory.auto_extract;
    let should_extract = save_schedules_extraction(
        &outcome,
        body.skip_extract,
        is_auto_extracted,
        auto_extract_enabled,
    );
    if should_extract {
        let svc = state.svc.clone();
        let id_for_extract = id.clone();
        let mem_cfg = state.svc.configuration().memory;
        let default_agent = state.svc.configuration().general_agent.agent_provider;
        tokio::task::spawn_blocking(move || {
            crate::mem::auto_extract::spawn_extraction_worker(
                &svc,
                &mem_cfg,
                &default_agent,
                id_for_extract,
                title_clone,
                body_clone,
                kind_for_extract,
            );
        });
    }

    Ok(Json(serde_json::json!({ "id": id, "outcome": outcome })))
}

pub async fn mem_update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    validate_update_body(&body).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let patch = MemUpdate {
        title: body.title,
        summary: body.summary,
        area: body.area,
        tags: body.tags,
        body: body.body,
        importance: body.importance,
        bump_recurrence: None,
        pinned: body.pinned,
    };
    let changed_by = body
        .changed_by
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(sanitize_actor)
        .unwrap_or_else(|| changed_by_from_headers(&headers));
    let updated =
        crate::mem::store::with_write_worker(super::worker_id_from_headers(&headers), || {
            crate::mem::store::with_changed_by(&changed_by, || state.svc.mem.update(&id, patch))
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !updated {
        if state
            .svc
            .mem
            .get(&id)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?
            .is_none()
        {
            return Err((StatusCode::NOT_FOUND, format!("mem item {} not found", id)));
        }
        return Ok(Json(serde_json::json!({ "unchanged": id })));
    }
    emit_activity(&state, &headers, "update", vec![id.clone()]);
    Ok(Json(serde_json::json!({ "updated": id })))
}

pub async fn mem_wake_up(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<WakeUpParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let caller_workspace = crate::api::tab_routes::caller_tab_header(&headers)
        .and_then(|tab_id| state.svc.mux.read().find_workspace_for_tab(&tab_id));
    let opts = wake_up_options_from_params(
        state.svc.configuration().memory.l1_budget_tokens,
        &p,
        caller_workspace,
    );
    let text = state
        .svc
        .mem
        .wake_up(&opts)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "text": text })))
}

pub async fn mem_stats(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let stats = state
        .svc
        .mem
        .stats()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(stats).unwrap_or_default()))
}

/// Bulk re-embed mem_items and/or graph entities into mem_vectors. Use this
/// after swapping the embedder model OR to backfill entities that pre-date
/// the auto-embed-on-create wiring (everything created before v1.1.23).
pub async fn mem_reembed(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<ReembedParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let scope = p.scope.as_deref().unwrap_or("all").to_lowercase();
    let do_items = matches!(scope.as_str(), "items" | "all");
    let do_entities = matches!(scope.as_str(), "entities" | "all");
    if !do_items && !do_entities {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("scope must be items | entities | all, got `{}`", scope),
        ));
    }

    let report = state
        .svc
        .mem
        .reembed_bulk(do_items, do_entities)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "scope": scope,
        "items_reembedded": report.items,
        "entities_reembedded": report.entities,
        "errors": report.errors,
    })))
}

pub async fn mem_learn(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<LearnBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let lk = match b.kind.as_deref() {
        Some("error") => LearnKind::Error,
        Some("feature") => LearnKind::Feature,
        _ => LearnKind::Insight,
    };
    let id = learnings::learn(&state.svc.mem, &b.text, lk, b.tags)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "id": id })))
}

pub async fn mem_promote(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let changed_by = changed_by_from_headers(&headers);
    crate::mem::store::with_changed_by(&changed_by, || state.svc.mem.promote_learning(&id))
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "promoted": id })))
}

pub async fn mem_check_duplicate(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<CheckDuplicateBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    if b.body.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "body required".into()));
    }
    let threshold = b.threshold.unwrap_or(0.85).clamp(0.0, 1.0);

    let raw = state
        .svc
        .mem
        .vector_recall(&b.body, 20)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let matches: Vec<serde_json::Value> = raw
        .into_iter()
        .filter_map(|(id, dist)| {
            let item = state.svc.mem.get(&id).ok()??;
            if let Some(ref k) = b.kind {
                if item.kind.as_str() != k.as_str() {
                    return None;
                }
            }
            if let Some(ref a) = b.area {
                if item.area.as_deref() != Some(a) {
                    return None;
                }
            }
            let similarity = (1.0 - (dist * dist) / 2.0).clamp(0.0, 1.0);
            if similarity < threshold {
                return None;
            }
            Some(serde_json::json!({
                "id": item.id,
                "title": item.title,
                "kind": item.kind.as_str(),
                "area": item.area,
                "similarity": similarity,
                "snippet": item.body.chars().take(200).collect::<String>(),
            }))
        })
        .take(5)
        .collect();

    Ok(Json(serde_json::json!({
        "is_duplicate": !matches.is_empty(),
        "threshold": threshold,
        "matches": matches,
    })))
}

pub async fn mem_history(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let history = state
        .svc
        .mem
        .get_item_history(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "history": history })))
}

/// Non-destructive grow: load the current body, append `text` with a
/// blank-line separator, persist via the normal update path (re-embeds +
/// logs a history row). Never clobbers the prior content.
pub async fn mem_append(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<AppendBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    if b.text.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "text required".into()));
    }
    let item = state
        .svc
        .mem
        .get(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?
        .ok_or((StatusCode::NOT_FOUND, format!("mem item {} not found", id)))?;
    let new_body = compose_appended_body(&item.body, &b.text);
    if new_body.len() > 1_048_576 {
        return Err((StatusCode::BAD_REQUEST, "body too large (max 1 MB)".into()));
    }
    let patch = MemUpdate {
        body: Some(new_body.clone()),
        ..Default::default()
    };
    let changed_by = b
        .changed_by
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(sanitize_actor)
        .unwrap_or_else(|| changed_by_from_headers(&headers));
    let updated =
        crate::mem::store::with_changed_by(&changed_by, || state.svc.mem.update(&id, patch))
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !updated {
        return Err((StatusCode::NOT_FOUND, format!("mem item {} not found", id)));
    }
    emit_activity(&state, &headers, "update", vec![id.clone()]);
    Ok(Json(
        serde_json::json!({ "appended": id, "new_len": new_body.len() }),
    ))
}

/// Recover from `mem_item_history`: an archived or merged item gets its prior
/// kind back; otherwise `version` (or the latest body change) is restored.
pub async fn mem_restore(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<RestoreBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let target = id.clone();
    let out = tokio::task::spawn_blocking(move || {
        crate::mem_ops::mem_merge::restore_item(&svc, &target, &b)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("join: {e}")))??;
    emit_activity(&state, &headers, "update", vec![id]);
    Ok(Json(out))
}

pub async fn mem_taxonomy(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<TaxonomyParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let tax = state
        .svc
        .mem
        .get_taxonomy(p.workspace.as_deref())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::to_value(tax).unwrap_or_default()))
}

pub async fn mem_tail(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<TailParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let limit = p.limit.unwrap_or(20).min(500);
    let rows = state
        .svc
        .mem
        .tail_recent(p.since.as_deref(), limit)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "rows": rows })))
}
