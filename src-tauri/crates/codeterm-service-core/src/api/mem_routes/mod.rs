use axum::http::{HeaderMap, StatusCode};

use crate::api::routes::AppState;
use crate::mem::activity::{now_ms, MemActivityEvent, MemActivitySource};

pub(super) const WORKER_ID_HEADER: &str = "x-codeterm-worker-id";
pub(super) const WORKER_INTENT_HEADER: &str = "x-codeterm-worker-intent";
pub(super) const ACTOR_HEADER: &str = "x-codeterm-actor";

pub mod area_merge;
pub mod calibration;
pub mod decision;
pub mod dream;
pub mod graph;
pub mod items;
pub mod layout;
pub mod mem_merge;
pub mod pinned;
pub mod search;
pub mod sync;
pub mod workers;

pub use calibration::{mem_calibration_log, mem_calibration_stale, mem_calibration_sweep};
pub use dream::{mem_dream, mem_dream_diary, mem_dream_report, mem_extract_pending};
pub use graph::{
    mem_graph_alias, mem_graph_collapse_synonym_edges, mem_graph_create, mem_graph_full,
    mem_graph_invalidate, mem_graph_path, mem_graph_prune_dream_hub_edges,
    mem_graph_prune_thin_vectors, mem_graph_relate, mem_graph_related,
    mem_graph_repair_augment_entity, mem_graph_repair_delete_entity,
    mem_graph_repair_delete_relations, mem_graph_repair_find_entities,
    mem_graph_repair_find_relations, mem_graph_repair_merge_relations,
    mem_graph_repair_orphan_blobs, mem_graph_repair_orphan_vectors, mem_graph_timeline,
    mem_graph_traverse, mem_graph_tunnels,
};
pub use items::{
    mem_activity_recent, mem_append, mem_archive, mem_check_duplicate, mem_delete, mem_forget_area,
    mem_get, mem_history, mem_learn, mem_pinned_list, mem_promote, mem_reembed, mem_restore,
    mem_save, mem_stats, mem_tail, mem_taxonomy, mem_update, mem_wake_up,
};
pub use layout::{mem_graph_edges, mem_graph_labels, mem_graph_layout_rebuild, mem_graph_snapshot};
pub use mem_merge::mem_merge;
pub use pinned::{
    mem_pinned_positions_delete, mem_pinned_positions_list, mem_pinned_positions_set,
};
pub use search::mem_search;
pub use sync::{mem_sync, mem_sync_status};
pub use workers::{
    cancel_worker, workers_get, workers_kill, workers_list, workers_log, workers_progress,
    workers_report, workers_spawn,
};

/// Exact Worker id a request is attributed to, for commit authorization. Absent
/// header means the write belongs to no extraction generation.
pub(super) fn worker_id_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers.get(WORKER_ID_HEADER).and_then(|v| v.to_str().ok())
}

pub(crate) fn validate_item_fields(
    title: &str,
    body: &str,
    area: &Option<String>,
    tags: &[String],
    importance: Option<f64>,
) -> Result<(), (StatusCode, String)> {
    codeterm_mem::curation::validate_item_fields(title, body, area, tags, importance).map_err(
        |(code, msg)| {
            (
                StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_REQUEST),
                msg,
            )
        },
    )
}

pub(crate) use codeterm_mem::curation::sanitize_actor;

/// Map incoming HTTP headers to a `changed_by` label for the history log.
/// - `x-codeterm-worker-id` present → `worker:<intent-or-other>`
/// - `x-codeterm-actor` present → sanitized value (reserved prefixes are
///   wrapped in `user-overridden:` to prevent audit-trail forgery)
/// - Otherwise → `user`
pub(super) fn changed_by_from_headers(headers: &HeaderMap) -> String {
    let worker_id = headers.get(WORKER_ID_HEADER).and_then(|v| v.to_str().ok());
    if let Some(_wid) = worker_id {
        let intent = headers
            .get(WORKER_INTENT_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("other");
        return format!("worker:{}", intent);
    }
    if let Some(actor) = headers.get(ACTOR_HEADER).and_then(|v| v.to_str().ok()) {
        return sanitize_actor(actor);
    }
    "user".to_string()
}

pub(super) fn emit_activity(state: &AppState, headers: &HeaderMap, op: &str, mem_ids: Vec<String>) {
    let tab_id = headers
        .get("x-codeterm-tab-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let worker_id = headers
        .get(WORKER_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let kind = if worker_id.is_some() {
        "worker"
    } else {
        "rest"
    }
    .to_string();

    let event = MemActivityEvent {
        op: op.to_string(),
        mem_ids,
        source: MemActivitySource {
            kind,
            tab_id,
            worker_id,
            agent_provider: None,
        },
        ts_ms: now_ms(),
    };

    state.svc.activity.record(event.clone());
    crate::api::events::emit_everywhere(&state.svc.app_handle, "mem-activity", &event);
}
