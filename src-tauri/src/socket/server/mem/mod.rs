//! Socket RPC surface for memory operations; validation is shared with HTTP routes.
//! Plaintext-capable secret operations retain bearer-authenticated HTTP.

mod graph;

use crate::api::mem_routes::items::{
    bump_on_get, compose_appended_body, pick_restore_body, validate_update_body,
    wake_up_options_from_params, AppendBody, FlushBody, LearnBody, PinnedListParams, ReembedParams,
    RestoreBody, SaveBody, TailParams, TaxonomyParams, UpdateBody, WakeUpParams,
};
use crate::api::mem_routes::search::{execute_search, SearchParams};
use crate::api::mem_routes::{sanitize_actor, validate_item_fields};
use crate::mem::activity::{now_ms, MemActivityEvent, MemActivitySource};
use crate::mem::learnings::{self, LearnKind};
use crate::mem::store::{with_changed_by, MemKind, MemScope, MemUpdate, NewMemItem};
use crate::service::ServiceState;

use super::{get_string_param, opt_string_param, JsonRpcError};
use crate::multiplexer::types::TabCloseReason;

pub(super) fn dispatch(
    method: &str,
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    if let Some(rejection) = secret_socket_rejection(method) {
        return Err(rejection);
    }
    if let Some(rest) = method.strip_prefix("mem.graph.") {
        return graph::dispatch(rest, params, svc);
    }
    match method {
        "mem.stats" => svc
            .mem
            .stats()
            .and_then(|stats| serde_json::to_value(stats).map_err(|error| error.to_string()))
            .map_err(internal_error),
        "mem.search" => {
            let request = serde_json::from_value::<SearchParams>(params.unwrap_or_default())
                .map_err(|error| {
                    invalid_params(format!("Invalid mem.search parameters: {error}"))
                })?;
            execute_search(svc, request).map_err(internal_error)
        }
        "mem.get" => mem_get(params, svc),
        "mem.save" => mem_save(params, svc),
        "mem.delete" => mem_delete(params, svc),
        "mem.forget" => mem_forget(params, svc),
        "mem.update" => mem_update(params, svc),
        "mem.pinned" => mem_pinned(params, svc),
        "mem.append" => mem_append(params, svc),
        "mem.restore" => mem_restore(params, svc),
        "mem.wake_up" => mem_wake_up(params, svc),
        "mem.learn" => mem_learn(params, svc),
        "mem.learn_promote" => mem_learn_promote(params, svc),
        "mem.flush" => mem_flush(params, svc),
        "mem.sync" => mem_sync(params, svc),
        "mem.reembed" => mem_reembed(params, svc),
        "mem.extract_pending" => mem_extract_pending(params, svc),
        "mem.history" => mem_history(params, svc),
        "mem.taxonomy" => mem_taxonomy(params, svc),
        "mem.tail" => mem_tail(params, svc),
        "mem.calibration.log" => mem_calibration_log(params, svc),
        "mem.calibration.sweep" => mem_calibration_sweep(params, svc),
        "mem.worker.spawn" => mem_worker_spawn(params, svc),
        "mem.worker.list" => mem_worker_list(svc),
        "mem.worker.get" => mem_worker_get(params, svc),
        "mem.worker.progress" => mem_worker_progress(params, svc),
        "mem.worker.report" => mem_worker_report(params, svc),
        "mem.worker.kill" => mem_worker_kill(params, svc),
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}

/// Reject secret RPC explicitly because this socket has no bearer-token gate.
fn secret_socket_rejection(method: &str) -> Option<JsonRpcError> {
    method.starts_with("mem.secret.").then(|| JsonRpcError {
        code: -32004,
        message: "mem secret is authenticated-HTTP-only by design: secrets never traverse the \
                   unauthenticated local socket. Use the HTTP API (/api/secrets/*)."
            .to_string(),
        data: None,
    })
}

pub(super) fn internal_error(message: String) -> JsonRpcError {
    JsonRpcError {
        code: -32000,
        message,
        data: None,
    }
}

pub(super) fn invalid_params(message: String) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message,
        data: None,
    }
}

pub(super) fn not_found(message: String) -> JsonRpcError {
    JsonRpcError {
        code: -32001,
        message,
        data: None,
    }
}

pub(super) fn parse_params<T: serde::de::DeserializeOwned>(
    params: Option<serde_json::Value>,
) -> Result<T, JsonRpcError> {
    serde_json::from_value(params.unwrap_or_default())
        .map_err(|error| invalid_params(format!("Invalid parameters: {error}")))
}

/// `changed_by` for CLI-originated writes: the CLI never sends the
/// worker/actor headers HTTP callers can, so absent an explicit
/// `--changed-by` this is always the plain "user" actor.
fn changed_by_or_user(changed_by: Option<String>) -> String {
    changed_by
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(sanitize_actor)
        .unwrap_or_else(|| "user".to_string())
}

fn emit_activity(svc: &ServiceState, op: &str, mem_ids: Vec<String>) {
    let event = MemActivityEvent {
        op: op.to_string(),
        mem_ids,
        source: MemActivitySource {
            kind: "cli".to_string(),
            tab_id: None,
            worker_id: None,
            agent_provider: None,
        },
        ts_ms: now_ms(),
    };
    svc.activity.record(event.clone());
    crate::api::events::emit_everywhere(&svc.app_handle, "mem-activity", &event);
}

fn mem_get(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let id = get_string_param(&params, "id")?;
    let item = svc.mem.get(&id).map_err(internal_error)?;
    match item {
        Some(item) => {
            emit_activity(svc, "read", vec![id.clone()]);
            svc.mem.with_conn(|conn| bump_on_get(conn, &item.kind, &id));
            serde_json::to_value(item).map_err(|error| internal_error(error.to_string()))
        }
        None => Err(not_found(format!("mem item {} not found", id))),
    }
}

fn mem_save(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let body: SaveBody = parse_params(params)?;
    validate_item_fields(
        &body.title,
        &body.body,
        &body.area,
        &body.tags,
        body.importance,
    )
    .map_err(|(_, message)| invalid_params(message))?;
    let kind = MemKind::from_str(&body.kind)
        .ok_or_else(|| invalid_params(format!("unknown kind: {}", body.kind)))?;
    let scope = body
        .scope
        .as_deref()
        .map(MemScope::from_str)
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
    let replace = body.replace;
    let skip_extract = body.skip_extract;
    let id = with_changed_by("user", || {
        if replace {
            svc.mem.save_replace(item)
        } else {
            svc.mem.save(item)
        }
    })
    .map_err(internal_error)?;
    emit_activity(svc, "write", vec![id.clone()]);

    let auto_extract_enabled = crate::config::memory_config().auto_extract;
    if !skip_extract && !is_auto_extracted && auto_extract_enabled {
        let mem_cfg = crate::config::memory_config();
        let default_agent = crate::config::default_agent_provider();
        crate::mem::auto_extract::spawn_extraction_worker(
            svc,
            &mem_cfg,
            &default_agent,
            id.clone(),
            title_clone,
            body_clone,
            kind_for_extract,
        );
    }
    Ok(serde_json::json!({ "id": id }))
}

fn mem_delete(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let id = get_string_param(&params, "id")?;
    let removed = with_changed_by("user", || svc.mem.delete(&id)).map_err(internal_error)?;
    emit_activity(svc, "delete", vec![id.clone()]);
    Ok(serde_json::json!({ "deleted": removed }))
}

fn mem_forget(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let area = get_string_param(&params, "area")?;
    let outcome = svc.mem.delete_by_area(&area).map_err(internal_error)?;
    emit_activity(svc, "delete", vec![format!("area:{}", area)]);
    Ok(serde_json::json!({
        "deleted": outcome.deleted,
        "area": area,
        "cancelled_extractions": outcome.cancelled_extractions,
    }))
}

fn mem_update(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let id = get_string_param(&params, "id")?;
    let body: UpdateBody = parse_params(params)?;
    validate_update_body(&body).map_err(invalid_params)?;
    let changed_by = changed_by_or_user(body.changed_by.clone());
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
    let updated =
        with_changed_by(&changed_by, || svc.mem.update(&id, patch)).map_err(internal_error)?;
    if !updated {
        return Err(not_found(format!("mem item {} not found", id)));
    }
    emit_activity(svc, "update", vec![id.clone()]);
    Ok(serde_json::json!({ "updated": id }))
}

fn mem_pinned(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let p: PinnedListParams = parse_params(params)?;
    let limit = p.limit.unwrap_or(100).clamp(1, 500);
    let items = svc.mem.list_pinned(limit).map_err(internal_error)?;
    Ok(serde_json::json!({ "items": items }))
}

fn mem_append(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let id = get_string_param(&params, "id")?;
    let body: AppendBody = parse_params(params)?;
    if body.text.trim().is_empty() {
        return Err(invalid_params("text required".into()));
    }
    let item = svc
        .mem
        .get(&id)
        .map_err(internal_error)?
        .ok_or_else(|| not_found(format!("mem item {} not found", id)))?;
    let new_body = compose_appended_body(&item.body, &body.text);
    if new_body.len() > 1_048_576 {
        return Err(invalid_params("body too large (max 1 MB)".into()));
    }
    let patch = MemUpdate {
        body: Some(new_body.clone()),
        ..Default::default()
    };
    let changed_by = changed_by_or_user(body.changed_by);
    let updated =
        with_changed_by(&changed_by, || svc.mem.update(&id, patch)).map_err(internal_error)?;
    if !updated {
        return Err(not_found(format!("mem item {} not found", id)));
    }
    emit_activity(svc, "update", vec![id.clone()]);
    Ok(serde_json::json!({ "appended": id, "new_len": new_body.len() }))
}

fn mem_restore(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let id = get_string_param(&params, "id")?;
    let body: RestoreBody = parse_params(params)?;
    let history = svc.mem.get_item_history(&id).map_err(internal_error)?;
    if history.is_empty() {
        return Err(not_found(format!("no history for {}", id)));
    }
    let restored_body = pick_restore_body(&history, body.version)
        .map_err(|(_, message)| invalid_params(message))?;
    let patch = MemUpdate {
        body: Some(restored_body.clone()),
        ..Default::default()
    };
    let changed_by = body
        .changed_by
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(sanitize_actor)
        .unwrap_or_else(|| "restore".to_string());
    let updated =
        with_changed_by(&changed_by, || svc.mem.update(&id, patch)).map_err(internal_error)?;
    if !updated {
        return Err(not_found(format!("mem item {} not found", id)));
    }
    emit_activity(svc, "update", vec![id.clone()]);
    Ok(serde_json::json!({ "restored": id, "body_len": restored_body.len() }))
}

fn mem_wake_up(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let p: WakeUpParams = parse_params(params)?;
    let opts = wake_up_options_from_params(&p, None);
    let text = svc.mem.wake_up(&opts).map_err(internal_error)?;
    Ok(serde_json::json!({ "text": text }))
}

fn mem_learn(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let b: LearnBody = parse_params(params)?;
    let lk = match b.kind.as_deref() {
        Some("error") => LearnKind::Error,
        Some("feature") => LearnKind::Feature,
        _ => LearnKind::Insight,
    };
    let id = learnings::learn(&svc.mem, &b.text, lk, b.tags).map_err(internal_error)?;
    Ok(serde_json::json!({ "id": id }))
}

fn mem_learn_promote(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let id = get_string_param(&params, "id")?;
    with_changed_by("user", || svc.mem.promote_learning(&id)).map_err(invalid_params)?;
    Ok(serde_json::json!({ "promoted": id }))
}

fn mem_flush(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let b: FlushBody = parse_params(params)?;
    let pty_id = {
        let mux = svc.mux.write();
        mux.resolve_tab_id(&b.tab)
            .ok_or_else(|| not_found(format!("Tab not found: {}", b.tab)))?
    };
    let pty_id_for_buffer = svc.resolve_pty_id(&pty_id).map_err(not_found)?;
    let body = {
        let buf = svc.output_buf.lock();
        buf.get_full_text(&pty_id_for_buffer).unwrap_or_default()
    };
    let id = learnings::flush_tab(&svc.mem, &b.tab, b.title.as_deref().unwrap_or(""), body)
        .map_err(invalid_params)?;
    Ok(serde_json::json!({ "id": id }))
}

fn mem_sync(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let p: crate::api::mem_routes::sync::SyncParams = parse_params(params)?;
    let result = if let Some(path) = p.path {
        crate::mem::file_sync::sync_path(&svc.mem, &path)
    } else {
        crate::mem::file_sync::sync_all(&svc.mem)
    }
    .map_err(internal_error)?;
    serde_json::to_value(&result).map_err(|error| internal_error(error.to_string()))
}

fn mem_reembed(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let p: ReembedParams = parse_params(params)?;
    let scope = p.scope.as_deref().unwrap_or("all").to_lowercase();
    let do_items = matches!(scope.as_str(), "items" | "all");
    let do_entities = matches!(scope.as_str(), "entities" | "all");
    if !do_items && !do_entities {
        return Err(invalid_params(format!(
            "scope must be items | entities | all, got `{}`",
            scope
        )));
    }
    let report = svc
        .mem
        .reembed_bulk(do_items, do_entities)
        .map_err(internal_error)?;
    Ok(serde_json::json!({
        "ok": true,
        "scope": scope,
        "items_reembedded": report.items,
        "entities_reembedded": report.entities,
        "errors": report.errors,
    }))
}

fn mem_extract_pending(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let p: crate::api::mem_routes::dream::ExtractPendingParams = parse_params(params)?;
    let limit = p.limit.unwrap_or(200).clamp(1, 1000);
    let count = crate::mem::auto_extract::run_extract_pending(svc.clone(), limit);
    Ok(serde_json::json!({ "queued": count }))
}

fn mem_history(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let id = get_string_param(&params, "id")?;
    let history = svc.mem.get_item_history(&id).map_err(internal_error)?;
    Ok(serde_json::json!({ "history": history }))
}

fn mem_taxonomy(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let p: TaxonomyParams = parse_params(params)?;
    let tax = svc
        .mem
        .get_taxonomy(p.workspace.as_deref())
        .map_err(internal_error)?;
    serde_json::to_value(tax).map_err(|error| internal_error(error.to_string()))
}

fn mem_tail(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let p: TailParams = parse_params(params)?;
    let limit = p.limit.unwrap_or(20).min(500);
    let rows = svc
        .mem
        .tail_recent(p.since.as_deref(), limit)
        .map_err(internal_error)?;
    Ok(serde_json::json!({ "rows": rows }))
}

fn mem_calibration_log(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    use crate::api::mem_routes::calibration::CalibLogParams;
    let p: CalibLogParams = parse_params(params)?;
    let limit = p.limit.unwrap_or(50).min(500);
    let since = match p.since.as_deref() {
        Some(s) if !s.is_empty() => {
            chrono::DateTime::parse_from_rfc3339(s)
                .map_err(|e| invalid_params(format!("since must be RFC3339: {}", e)))?;
            s.to_string()
        }
        _ => "1970-01-01T00:00:00Z".to_string(),
    };
    let count_only = p.count.unwrap_or(false);
    svc.mem
        .with_conn(|conn| {
            crate::api::mem_routes::calibration::calibration_log_query(
                conn, &since, limit, count_only,
            )
        })
        .map_err(internal_error)
}

fn mem_calibration_sweep(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    use crate::api::mem_routes::calibration::SweepParams;
    let p: SweepParams = parse_params(params)?;
    let force = p.force.unwrap_or(false);
    svc.mem
        .with_conn(|conn| crate::api::mem_routes::calibration::run_calibration_sweep(conn, force))
        .map_err(internal_error)
}

fn mem_worker_spawn(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    use crate::mem::workers;
    #[derive(serde::Deserialize)]
    struct SpawnParams {
        intent: String,
        task: String,
        #[serde(default)]
        agent: Option<String>,
        #[serde(default)]
        parent: Option<String>,
    }
    let b: SpawnParams = parse_params(params)?;
    let mem_cfg = crate::config::memory_config();
    let parent = b.parent.unwrap_or_else(|| "user:memory-panel".to_string());
    let resolved = codeterm_mem::worker_agent::resolve_worker(
        b.agent.as_deref(),
        &mem_cfg.extraction_agent,
        &mem_cfg.extraction_model,
        &crate::config::default_agent_provider(),
        &mem_cfg.worker_model,
        &crate::service::spawn::CatalogBackgroundModelPort,
    );
    let worker_id = workers::spawn_headless(
        svc,
        b.intent,
        b.task,
        parent,
        resolved.agent,
        resolved.model,
    )
    .map_err(internal_error)?;
    let info = svc.workers.get(&worker_id);
    Ok(serde_json::json!({
        "worker_id": worker_id,
        "tab_id": info.as_ref().map(|i| &i.tab_id),
        "info": info,
    }))
}

fn mem_worker_list(svc: &ServiceState) -> Result<serde_json::Value, JsonRpcError> {
    serde_json::to_value(svc.workers.list()).map_err(|error| internal_error(error.to_string()))
}

fn mem_worker_get(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let id = get_string_param(&params, "id")?;
    svc.workers
        .get(&id)
        .ok_or_else(|| not_found(format!("worker {} not found", id)))
        .and_then(|info| {
            serde_json::to_value(info).map_err(|error| internal_error(error.to_string()))
        })
}

fn mem_worker_progress(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    use crate::mem::workers::WorkerStatus;
    let id = get_string_param(&params, "id")?;
    #[derive(serde::Deserialize)]
    struct ProgressParams {
        text: String,
        #[serde(default)]
        status: Option<String>,
    }
    let b: ProgressParams = parse_params(params)?;
    svc.workers
        .update_progress(&id, b.text)
        .map_err(not_found)?;
    if let Some(s) = b.status.as_deref().and_then(WorkerStatus::from_str) {
        svc.workers.update_status(&id, s).map_err(not_found)?;
    }
    if let Some(info) = svc.workers.get(&id) {
        crate::api::events::emit_everywhere(&svc.app_handle, "graph-worker-progress", &info);
    }
    Ok(serde_json::json!({ "ok": true }))
}

fn mem_worker_report(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let id = get_string_param(&params, "id")?;
    let summary = get_string_param(&params, "summary")?;
    let reported = crate::mem::dream_runs::ReportedFinalization {
        diary_id: opt_string_param(&params, "diary"),
        claim: opt_string_param(&params, "finalization"),
        reason: opt_string_param(&params, "reason"),
    };
    let worker_tab_id = svc.workers.get(&id).map(|w| w.tab_id.clone());
    let (pty_id, session_id, run_cwd) = worker_tab_id
        .as_ref()
        .map(|pid| {
            let mux = svc.mux.write();
            let tab = mux.get_tab(pid);
            (
                tab.and_then(|p| p.pty_id.clone()),
                tab.and_then(|p| p.agent_session_id.clone()),
                tab.and_then(|p| p.cwd.clone()),
            )
        })
        .unwrap_or((None, None, None));
    if let Some(ref sid) = session_id {
        svc.workers.set_run_session(&id, sid.clone(), run_cwd);
    }
    if let Some(ref pty_id) = pty_id {
        let output = svc
            .output_buf
            .lock()
            .get_full_text(pty_id)
            .unwrap_or_default();
        if !output.is_empty() {
            match crate::mem::workers::save_worker_log(&id, &output) {
                Ok(path) => svc.workers.set_log_path(&id, path.display().to_string()),
                Err(e) => log::warn!("[graph-workers] save log: {}", e),
            }
        }
        svc.run_watcher.mark_completed_externally(pty_id);
    }
    crate::mem::dream_runs::record_reported_finalization(svc, &id, &reported);
    let info = svc.workers.complete(&id, summary).map_err(not_found)?;
    crate::api::events::emit_everywhere(&svc.app_handle, "graph-worker-reported", &info);
    let svc_close = svc.clone();
    let tab_id = info.tab_id.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        if let Err(e) = svc_close.close_tab_with_reason(&tab_id, TabCloseReason::AgentRunEnded) {
            log::warn!("[graph-workers] auto-close Tab {}: {}", tab_id, e);
        }
    });
    Ok(serde_json::json!({ "ok": true, "info": info }))
}

fn mem_worker_kill(
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let id = get_string_param(&params, "id")?;
    let info = svc.workers.kill(&id).map_err(not_found)?;
    if let Err(e) = svc.close_tab_with_reason(&info.tab_id, TabCloseReason::Cancelled) {
        log::warn!("[graph-workers] kill Tab {}: {}", info.tab_id, e);
    }
    crate::api::events::emit_everywhere(&svc.app_handle, "graph-worker-done", &info);
    Ok(serde_json::json!({ "ok": true }))
}

#[cfg(test)]
mod tests {
    use super::secret_socket_rejection;

    #[test]
    fn secret_methods_are_explicitly_rejected_over_socket() {
        for method in [
            "mem.secret.save",
            "mem.secret.get",
            "mem.secret.list",
            "mem.secret.delete",
            "mem.secret.status",
            "mem.secret.unlock",
        ] {
            let err = secret_socket_rejection(method)
                .unwrap_or_else(|| panic!("{method} must be rejected over socket"));
            assert_eq!(err.code, -32004);
            assert!(err.message.contains("authenticated-HTTP-only"));
            assert!(err.message.contains("/api/secrets/"));
        }
    }

    #[test]
    fn non_secret_methods_are_not_rejected() {
        for method in ["mem.get", "mem.save", "mem.graph.create", "mem.stats"] {
            assert!(
                secret_socket_rejection(method).is_none(),
                "{method} should not hit the secret rejection"
            );
        }
    }
}
