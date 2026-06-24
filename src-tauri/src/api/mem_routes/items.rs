use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::mem::activity::now_ms;
use crate::mem::{
    layers::WakeUpOptions,
    learnings::{self, LearnKind},
    store::{MemKind, MemScope, MemUpdate, NewMemItem},
};

use super::{
    changed_by_from_headers, emit_activity, sanitize_actor, validate_item_fields, WORKER_ID_HEADER,
};
#[derive(Deserialize)]
pub struct ActivityRecentParams {
    #[serde(default)]
    pub since_ms: Option<u64>,
}

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

#[derive(Deserialize)]
pub struct PinnedListParams {
    #[serde(default)]
    pub limit: Option<usize>,
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

/// Bumps `evidence_count` for L1-selectable kinds (skill|lesson|rule|fact|preference); no-op otherwise. Caller decides whether to invoke.
fn bump_on_get(conn: &rusqlite::Connection, kind: &str, id: &str) {
    if !matches!(kind, "skill" | "lesson" | "rule" | "fact" | "preference") {
        return;
    }
    if let Err(e) = codeterm_mem_store::graph::bump_evidence_count(conn, id) {
        log::debug!("[mem_get] evidence bump failed for {}: {}", id, e);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bump_on_get, compose_appended_body, pick_restore_body, wake_up_options_from_params,
        WakeUpParams,
    };
    use crate::mem::store::HistoryEntry;
    use rusqlite::Connection;

    fn hist(id: i64, old: Option<&str>, new: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            id,
            item_id: "x".into(),
            changed_at: "t".into(),
            changed_by: "user".into(),
            change_type: "update".into(),
            old_title: None,
            new_title: None,
            old_body: old.map(String::from),
            new_body: new.map(String::from),
            old_importance: None,
            new_importance: None,
            old_kind: None,
            new_kind: None,
            reason: None,
        }
    }

    #[test]
    fn append_to_empty_replaces_outright() {
        assert_eq!(compose_appended_body("", "section"), "section");
        assert_eq!(compose_appended_body("   \n", "section"), "section");
    }

    #[test]
    fn append_separates_with_blank_line_and_keeps_old() {
        let out = compose_appended_body("old body", "new section");
        assert_eq!(out, "old body\n\nnew section");
        assert!(out.starts_with("old body"));
    }

    #[test]
    fn append_collapses_trailing_newlines_before_separator() {
        assert_eq!(compose_appended_body("old\n\n", "new"), "old\n\nnew");
    }

    #[test]
    fn restore_default_picks_latest_old_body() {
        let h = vec![
            hist(1, None, Some("v1")),
            hist(2, Some("v1"), Some("v2")),
            hist(3, Some("v2"), Some("v3")),
        ];
        assert_eq!(pick_restore_body(&h, None).unwrap(), "v2");
    }

    #[test]
    fn restore_version_picks_that_rows_new_body() {
        let h = vec![hist(1, None, Some("v1")), hist(2, Some("v1"), Some("v2"))];
        assert_eq!(pick_restore_body(&h, Some(1)).unwrap(), "v1");
        assert_eq!(pick_restore_body(&h, Some(2)).unwrap(), "v2");
    }

    #[test]
    fn restore_unknown_version_errors() {
        let h = vec![hist(1, None, Some("v1"))];
        assert!(pick_restore_body(&h, Some(99)).is_err());
    }

    #[test]
    fn restore_with_no_body_history_errors() {
        let h = vec![hist(1, None, None)];
        assert!(pick_restore_body(&h, None).is_err());
    }

    fn open_test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::mem::store::init_schema(&conn).unwrap();
        conn
    }

    fn insert(conn: &Connection, id: &str, kind: &str) {
        conn.execute(
            "INSERT INTO mem_items (id, uid, kind, scope, title, tags_json, created_at, updated_at)
             VALUES (?1, ?1, ?2, 'global', 't', '[]', '2026-01-01', '2026-01-01')",
            rusqlite::params![id, kind],
        )
        .unwrap();
    }

    fn evidence(conn: &Connection, id: &str) -> i64 {
        conn.query_row(
            "SELECT evidence_count FROM mem_items WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn bump_on_get_increments_for_lesson() {
        let conn = open_test_conn();
        insert(&conn, "lesson_a", "lesson");
        bump_on_get(&conn, "lesson", "lesson_a");
        assert_eq!(evidence(&conn, "lesson_a"), 2, "default 1 + 1 bump = 2");
    }

    #[test]
    fn bump_on_get_increments_for_skill() {
        let conn = open_test_conn();
        insert(&conn, "skill_a", "skill");
        bump_on_get(&conn, "skill", "skill_a");
        assert_eq!(evidence(&conn, "skill_a"), 2);
    }

    #[test]
    fn bump_on_get_skips_other_kinds() {
        let conn = open_test_conn();
        insert(&conn, "note_a", "note");
        bump_on_get(&conn, "note", "note_a");
        assert_eq!(evidence(&conn, "note_a"), 1, "kind=note must not bump");
    }

    #[test]
    fn bump_on_get_fires_for_fact() {
        let conn = open_test_conn();
        insert(&conn, "fact_a", "fact");
        bump_on_get(&conn, "fact", "fact_a");
        assert_eq!(evidence(&conn, "fact_a"), 2);
    }

    #[test]
    fn bump_on_get_fires_for_rule() {
        let conn = open_test_conn();
        insert(&conn, "rule_a", "rule");
        bump_on_get(&conn, "rule", "rule_a");
        assert_eq!(evidence(&conn, "rule_a"), 2);
    }

    #[test]
    fn bump_on_get_fires_for_preference() {
        let conn = open_test_conn();
        insert(&conn, "pref_a", "preference");
        bump_on_get(&conn, "preference", "pref_a");
        assert_eq!(evidence(&conn, "pref_a"), 2);
    }

    #[test]
    fn bump_on_get_is_safe_when_row_missing() {
        let conn = open_test_conn();
        // No insert — must not panic / error.
        bump_on_get(&conn, "skill", "ghost_id");
    }

    #[test]
    fn wake_up_budget_below_l0_floor_does_not_underflow() {
        let opts = wake_up_options_from_params(&WakeUpParams {
            budget: Some(10),
            l0_only: None,
            no_l0: None,
            explain: None,
            workspace: None,
        });

        assert_eq!(opts.l0_budget_tokens, 10);
        assert_eq!(opts.l1_budget_tokens, 0);
    }

    #[test]
    fn wake_up_no_l0_uses_full_budget_for_l1() {
        let opts = wake_up_options_from_params(&WakeUpParams {
            budget: Some(10),
            l0_only: None,
            no_l0: Some(true),
            explain: None,
            workspace: None,
        });

        assert_eq!(opts.l0_budget_tokens, 0);
        assert_eq!(opts.l1_budget_tokens, 10);
        assert!(!opts.include_l0);
        assert!(opts.include_l1);
    }

    #[test]
    fn wake_up_large_budget_caps_l0_share() {
        let opts = wake_up_options_from_params(&WakeUpParams {
            budget: Some(2_000),
            l0_only: None,
            no_l0: None,
            explain: None,
            workspace: None,
        });

        assert_eq!(opts.l0_budget_tokens, 100);
        assert_eq!(opts.l1_budget_tokens, 1_900);
    }

    #[test]
    fn wake_up_no_budget_uses_config_default() {
        let opts = wake_up_options_from_params(&WakeUpParams {
            budget: None,
            l0_only: None,
            no_l0: None,
            explain: None,
            workspace: None,
        });
        assert_eq!(
            opts.l1_budget_tokens,
            crate::config::schema::MemoryConfig::default().l1_budget_tokens
        );
    }

    #[test]
    fn wake_up_workspace_param_propagates_to_options() {
        let opts = wake_up_options_from_params(&WakeUpParams {
            budget: None,
            l0_only: None,
            no_l0: None,
            explain: None,
            workspace: Some("W1".into()),
        });
        assert_eq!(opts.current_workspace.as_deref(), Some("W1"));
    }
}

#[derive(Deserialize)]
pub struct SaveBody {
    pub kind: String,
    #[serde(default)]
    pub scope: Option<String>,
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub body: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub importance: Option<f64>,
    #[serde(default)]
    pub replace: bool,
    #[serde(default)]
    pub skip_extract: bool,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub pinned: Option<bool>,
}

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
        .map(|s| MemScope::from_str(&s))
        .unwrap_or(MemScope::Global);
    // Check BEFORE moving fields — prevents extraction cascade
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
    let id = crate::mem::store::with_changed_by(&changed_by, || {
        if body.replace {
            state.svc.mem.save_replace(item)
        } else {
            state.svc.mem.save(item)
        }
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    emit_activity(&state, &headers, "write", vec![id.clone()]);

    let auto_extract_enabled = crate::config::memory_config().auto_extract;
    let should_extract = !body.skip_extract && !is_auto_extracted && auto_extract_enabled;
    if should_extract {
        let svc = state.svc.clone();
        let id_for_extract = id.clone();
        tokio::task::spawn_blocking(move || {
            crate::mem::auto_extract::spawn_extraction_worker(
                &svc,
                id_for_extract,
                title_clone,
                body_clone,
                kind_for_extract,
            );
        });
    }

    Ok(Json(serde_json::json!({ "id": id })))
}

pub async fn mem_delete(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let changed_by = changed_by_from_headers(&headers);
    let removed = crate::mem::store::with_changed_by(&changed_by, || state.svc.mem.delete(&id))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    emit_activity(&state, &headers, "delete", vec![id.clone()]);
    Ok(Json(serde_json::json!({ "deleted": removed })))
}

pub async fn mem_forget_area(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(area): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let count = state
        .svc
        .mem
        .delete_by_area(&area)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    emit_activity(&state, &headers, "delete", vec![format!("area:{}", area)]);
    Ok(Json(serde_json::json!({ "deleted": count, "area": area })))
}

#[derive(Deserialize, Default)]
pub struct UpdateBody {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub importance: Option<f64>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub changed_by: Option<String>,
}

pub async fn mem_update(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    if let Some(ref t) = body.title {
        let tt = t.trim();
        if tt.is_empty() || tt.len() < 3 || tt.len() > 200 {
            return Err((StatusCode::BAD_REQUEST, "title must be 3..200 chars".into()));
        }
    }
    if let Some(ref b) = body.body {
        if b.len() > 1_048_576 {
            return Err((StatusCode::BAD_REQUEST, "body too large (max 1 MB)".into()));
        }
    }
    if let Some(ref a) = body.area {
        if !a.is_empty()
            && (a.len() > 64
                || !a
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        {
            return Err((StatusCode::BAD_REQUEST, format!("invalid area: `{}`", a)));
        }
    }
    if let Some(ref ts) = body.tags {
        if ts.len() > 20 {
            return Err((StatusCode::BAD_REQUEST, "too many tags (max 20)".into()));
        }
        for t in ts {
            if t.len() > 50 {
                return Err((StatusCode::BAD_REQUEST, format!("tag too long: `{}`", t)));
            }
        }
    }
    if let Some(i) = body.importance {
        if !(0.0..=5.0).contains(&i) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("importance must be 0..5, got {}", i),
            ));
        }
    }
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
        crate::mem::store::with_changed_by(&changed_by, || state.svc.mem.update(&id, patch))
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !updated {
        return Err((StatusCode::NOT_FOUND, format!("mem item {} not found", id)));
    }
    emit_activity(&state, &headers, "update", vec![id.clone()]);
    Ok(Json(serde_json::json!({ "updated": id })))
}

#[derive(Deserialize)]
pub struct WakeUpParams {
    #[serde(default)]
    pub budget: Option<usize>,
    #[serde(default)]
    pub l0_only: Option<bool>,
    #[serde(default)]
    pub no_l0: Option<bool>,
    #[serde(default)]
    pub explain: Option<bool>,
    #[serde(default)]
    pub workspace: Option<String>,
}

pub async fn mem_wake_up(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<WakeUpParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let opts = wake_up_options_from_params(&p);
    let text = state
        .svc
        .mem
        .wake_up(&opts)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(serde_json::json!({ "text": text })))
}

fn wake_up_options_from_params(p: &WakeUpParams) -> WakeUpOptions {
    let mut opts = WakeUpOptions::default();
    opts.include_l0 = !p.no_l0.unwrap_or(false);
    opts.include_l1 = !p.l0_only.unwrap_or(false);
    opts.explain = p.explain.unwrap_or(false);
    opts.current_workspace = p.workspace.clone();

    if let Some(total) = p.budget {
        let total = total.max(1);
        match (opts.include_l0, opts.include_l1) {
            (true, true) => {
                let l0 = (total / 8).clamp(50, 100).min(total);
                opts.l0_budget_tokens = l0;
                opts.l1_budget_tokens = total.saturating_sub(l0);
            }
            (true, false) => {
                opts.l0_budget_tokens = total;
                opts.l1_budget_tokens = 0;
            }
            (false, true) => {
                opts.l0_budget_tokens = 0;
                opts.l1_budget_tokens = total;
            }
            (false, false) => {
                opts.l0_budget_tokens = 0;
                opts.l1_budget_tokens = 0;
            }
        }
    } else {
        opts.l1_budget_tokens = crate::config::memory_config().l1_budget_tokens;
    }

    opts
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

#[derive(Deserialize)]
pub struct ReembedParams {
    /// items | entities | all (default).
    #[serde(default)]
    pub scope: Option<String>,
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

#[derive(Deserialize)]
pub struct LearnBody {
    pub text: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
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

#[derive(Deserialize)]
pub struct FlushBody {
    pub pane: String,
    #[serde(default)]
    pub title: Option<String>,
}

pub async fn mem_flush(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(b): Json<FlushBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let pty_id = {
        let mux = state.svc.mux.write();
        mux.resolve_pane_id(&b.pane)
            .ok_or((StatusCode::NOT_FOUND, format!("pane not found: {}", b.pane)))?
    };
    let pty_id_for_buffer = state
        .svc
        .resolve_pty_id(&pty_id)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    let body = {
        let buf = state.svc.output_buf.lock();
        buf.get_full_text(&pty_id_for_buffer).unwrap_or_default()
    };
    let id = learnings::flush_pane(
        &state.svc.mem,
        &b.pane,
        b.title.as_deref().unwrap_or(""),
        body,
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(serde_json::json!({ "id": id })))
}

#[derive(Deserialize)]
pub struct CheckDuplicateBody {
    pub body: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub threshold: Option<f64>,
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

/// Join an existing body and appended text with a blank-line separator.
/// Empty/whitespace-only bodies are replaced outright (no leading blank lines).
fn compose_appended_body(old: &str, text: &str) -> String {
    if old.trim().is_empty() {
        text.to_string()
    } else {
        format!("{}\n\n{}", old.trim_end_matches('\n'), text)
    }
}

/// Choose the body to restore from an item's history. `version` targets a
/// specific history row id (its `new_body`, falling back to `old_body`);
/// `None` undoes the most recent body change (latest non-null `old_body`).
fn pick_restore_body(
    history: &[crate::mem::store::HistoryEntry],
    version: Option<i64>,
) -> Result<String, (StatusCode, String)> {
    match version {
        Some(v) => {
            let entry = history.iter().find(|h| h.id == v).ok_or((
                StatusCode::NOT_FOUND,
                format!("history version {} not found", v),
            ))?;
            entry
                .new_body
                .clone()
                .or_else(|| entry.old_body.clone())
                .ok_or((
                    StatusCode::BAD_REQUEST,
                    format!("history row {} has no body snapshot", v),
                ))
        }
        None => history
            .iter()
            .rev()
            .find_map(|h| h.old_body.clone())
            .ok_or((
                StatusCode::BAD_REQUEST,
                "no prior body to restore".to_string(),
            )),
    }
}

#[derive(Deserialize)]
pub struct AppendBody {
    pub text: String,
    #[serde(default)]
    pub changed_by: Option<String>,
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

#[derive(Deserialize)]
pub struct RestoreBody {
    #[serde(default)]
    pub version: Option<i64>,
    #[serde(default)]
    pub changed_by: Option<String>,
}

/// Recover a prior body from `mem_item_history`. With `version` set, restore
/// the body snapshot of that history row; otherwise undo the most recent body
/// change (revert to the latest `old_body`).
pub async fn mem_restore(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<RestoreBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let history = state
        .svc
        .mem
        .get_item_history(&id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if history.is_empty() {
        return Err((StatusCode::NOT_FOUND, format!("no history for {}", id)));
    }
    let restored_body = pick_restore_body(&history, b.version)?;
    let patch = MemUpdate {
        body: Some(restored_body.clone()),
        ..Default::default()
    };
    let changed_by = b
        .changed_by
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(sanitize_actor)
        .unwrap_or_else(|| "restore".to_string());
    let updated =
        crate::mem::store::with_changed_by(&changed_by, || state.svc.mem.update(&id, patch))
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !updated {
        return Err((StatusCode::NOT_FOUND, format!("mem item {} not found", id)));
    }
    emit_activity(&state, &headers, "update", vec![id.clone()]);
    Ok(Json(
        serde_json::json!({ "restored": id, "body_len": restored_body.len() }),
    ))
}

#[derive(Deserialize)]
pub struct TaxonomyParams {
    #[serde(default)]
    pub workspace: Option<String>,
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

#[derive(Deserialize)]
pub struct TailParams {
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
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
