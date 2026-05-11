use axum::http::{HeaderMap, StatusCode};
use tauri::Emitter;

use crate::api::routes::AppState;
use crate::mem::activity::{now_ms, MemActivityEvent, MemActivitySource};

pub(super) const WORKER_ID_HEADER: &str = "x-codeterm-worker-id";
pub(super) const WORKER_INTENT_HEADER: &str = "x-codeterm-worker-intent";
pub(super) const ACTOR_HEADER: &str = "x-codeterm-actor";

pub mod calibration;
pub mod dream;
pub mod graph;
pub mod items;
pub mod pinned;
pub mod sync;
pub mod workers;

pub use calibration::{mem_calibration_log, mem_calibration_sweep};
pub use dream::{mem_dream, mem_dream_report, mem_extract_pending};
pub use graph::{
    mem_graph_alias, mem_graph_create, mem_graph_full, mem_graph_invalidate, mem_graph_path,
    mem_graph_relate, mem_graph_related, mem_graph_timeline, mem_graph_traverse, mem_graph_tunnels,
};
pub use items::{
    mem_activity_recent, mem_check_duplicate, mem_delete, mem_flush, mem_forget_area, mem_get,
    mem_history, mem_learn, mem_pinned_list, mem_promote, mem_reembed, mem_save, mem_search,
    mem_stats, mem_tail, mem_taxonomy, mem_update, mem_wake_up,
};
pub use pinned::{mem_pinned_positions_delete, mem_pinned_positions_list, mem_pinned_positions_set};
pub use sync::{mem_sync, mem_sync_status};
pub use workers::{
    workers_get, workers_kill, workers_list, workers_log, workers_progress, workers_report,
    workers_spawn,
};

pub(super) fn validate_item_fields(
    title: &str,
    body: &str,
    area: &Option<String>,
    tags: &[String],
    importance: Option<f64>,
) -> Result<(), (StatusCode, String)> {
    let t = title.trim();
    if t.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "title must not be empty".into()));
    }
    if t.len() < 3 {
        return Err((StatusCode::BAD_REQUEST, "title must be >= 3 chars".into()));
    }
    if t.len() > 200 {
        return Err((StatusCode::BAD_REQUEST, "title must be <= 200 chars".into()));
    }
    if body.len() > 1_048_576 {
        return Err((StatusCode::BAD_REQUEST, format!("body too large: {} bytes (max 1 MB)", body.len())));
    }
    if let Some(a) = area.as_deref() {
        if !a.is_empty() {
            if a.len() > 64 {
                return Err((StatusCode::BAD_REQUEST, "area must be <= 64 chars".into()));
            }
            if !a.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                return Err((StatusCode::BAD_REQUEST, format!("area must match [a-zA-Z0-9_-]+: `{}`", a)));
            }
        }
    }
    if tags.len() > 20 {
        return Err((StatusCode::BAD_REQUEST, "too many tags (max 20)".into()));
    }
    for tag in tags {
        if tag.len() > 50 {
            return Err((StatusCode::BAD_REQUEST, format!("tag too long (max 50): `{}`", tag)));
        }
    }
    if let Some(i) = importance {
        if !(0.0..=5.0).contains(&i) {
            return Err((StatusCode::BAD_REQUEST, format!("importance must be 0.0..=5.0, got {}", i)));
        }
    }
    Ok(())
}

/// Reserved `changed_by` prefixes — only the server itself writes these.
/// Any client-supplied actor matching one of these is namespaced under
/// `user-overridden:` so it can never spoof a real worker / dreamer run
/// (which would trip the calibration re-entry guard or pollute the audit).
const RESERVED_CHANGED_BY_PREFIXES: &[&str] =
    &["worker:", "dreamer-", "dreaming:", "system:"];

pub(super) fn sanitize_actor(actor: &str) -> String {
    if RESERVED_CHANGED_BY_PREFIXES
        .iter()
        .any(|p| actor.starts_with(p))
    {
        format!("user-overridden:{actor}")
    } else {
        actor.to_string()
    }
}

/// Map incoming HTTP headers to a `changed_by` label for the history log.
/// - `x-codeterm-worker-id` present → `worker:<intent-or-other>`
/// - `x-codeterm-actor` present → sanitized value (reserved prefixes are
///   wrapped in `user-overridden:` to prevent audit-trail forgery)
/// - Otherwise → `user`
pub(super) fn changed_by_from_headers(headers: &HeaderMap) -> String {
    let worker_id = headers
        .get(WORKER_ID_HEADER)
        .and_then(|v| v.to_str().ok());
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

#[cfg(test)]
mod actor_sanitizer_tests {
    use super::sanitize_actor;

    #[test]
    fn plain_actor_passes_through() {
        assert_eq!(sanitize_actor("alice"), "alice");
        assert_eq!(sanitize_actor("cli:mem-pin"), "cli:mem-pin");
    }

    #[test]
    fn reserved_prefixes_are_wrapped() {
        assert_eq!(
            sanitize_actor("dreamer-calibration:promote"),
            "user-overridden:dreamer-calibration:promote",
        );
        assert_eq!(sanitize_actor("worker:rem"), "user-overridden:worker:rem");
        assert_eq!(
            sanitize_actor("dreaming:rem"),
            "user-overridden:dreaming:rem",
        );
        assert_eq!(
            sanitize_actor("system:dreaming"),
            "user-overridden:system:dreaming",
        );
    }
}

pub(super) fn emit_activity(state: &AppState, headers: &HeaderMap, op: &str, mem_ids: Vec<String>) {
    let pane_id = headers
        .get("x-codeterm-pane-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let worker_id = headers
        .get(WORKER_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let kind = if worker_id.is_some() { "worker" } else { "rest" }.to_string();

    let event = MemActivityEvent {
        op: op.to_string(),
        mem_ids,
        source: MemActivitySource {
            kind,
            pane_id,
            worker_id,
            agent_provider: None,
        },
        ts_ms: now_ms(),
    };

    state.svc.activity.record(event.clone());
    let _ = state.svc.app_handle.emit("mem-activity", &event);
}
