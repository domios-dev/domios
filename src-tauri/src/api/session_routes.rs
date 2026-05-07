use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};

#[derive(Deserialize)]
pub(crate) struct SearchSessionsQuery {
    q: String,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    max_snippets: Option<usize>,
}

/// Cross-provider session search: walks every registered `AgentProvider`,
/// collects past sessions from disk, and returns matches ranked by recency.
/// Each hit carries its `provider` so callers can route a "resume in new pane"
/// action to the correct launcher.
pub async fn search_sessions_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<SearchSessionsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let terms: Vec<String> = q
        .q
        .split_whitespace()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if terms.is_empty() {
        return Ok(Json(serde_json::json!({ "hits": [] })));
    }
    let opts = crate::agent::registry::SessionSearchOpts {
        terms,
        provider: q.provider.filter(|p| p != "all" && !p.is_empty()),
        project_path: q.project,
        limit: q.limit.unwrap_or(10),
        max_snippets: q.max_snippets.unwrap_or(3),
    };
    let hits = state.svc.agent_registry.search_sessions(&opts);
    Ok(Json(serde_json::json!({ "hits": hits })))
}

/// Resolve a session id to its metadata across all providers.
/// Returns `null` + 404 when nothing claims the id.
pub async fn get_session_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    match state.svc.agent_registry.find_session(&session_id) {
        Some(meta) => Ok(Json(serde_json::to_value(meta).unwrap_or_default())),
        None => Err((StatusCode::NOT_FOUND, format!("session {} not found", session_id))),
    }
}

#[derive(Deserialize)]
pub(crate) struct ResumeSessionBody {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    workspace_name: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

/// Atomic "resume a past session" action:
///   1. locate the session across providers → get cwd + provider
///   2. create (or pick) a workspace whose cwd matches that project path
///   3. create a tab + PTY in that workspace
///   4. wait for shell prompt + send the provider's resume command
pub async fn resume_session_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    body: Option<Json<ResumeSessionBody>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let body = body.map(|Json(v)| v).unwrap_or(ResumeSessionBody {
        title: None, workspace_id: None, workspace_name: None, model: None,
    });

    let meta = state
        .svc
        .agent_registry
        .find_session(&session_id)
        .ok_or((StatusCode::NOT_FOUND, format!("session {} not found", session_id)))?;

    let cwd = meta.project_path.clone().ok_or((
        StatusCode::BAD_REQUEST,
        format!("session {} has no recorded project_path; can't decide cwd", session_id),
    ))?;

    let resume_cmd_preview = state
        .svc
        .agent_registry
        .with_provider(&meta.provider, |p| p.build_resume_command(&meta.session_id, true))
        .flatten();

    let ws_id = match body.workspace_id {
        Some(id) => id,
        None => {
            let ws_name = body.workspace_name.unwrap_or_else(|| {
                std::path::Path::new(&cwd)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&meta.provider)
                    .to_string()
            });
            let cmd = crate::dispatch::Command::WorkspaceCreate {
                name: ws_name,
                cwd: Some(cwd.clone()),
            };
            let result = crate::dispatch::dispatch(&cmd, &state.svc)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
            result
                .into_json()
                .get("id")
                .and_then(|v| v.as_str())
                .map(String::from)
                .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "workspace create returned no id".into()))?
        }
    };

    let spawn_params = crate::service::SpawnAgentParams {
        agent: Some(meta.provider.clone()),
        agent_name: None,
        args: None,
        cwd: Some(cwd.clone()),
        title: body.title.or_else(|| {
            meta.title.clone().or_else(|| Some(format!("resumed {}", &meta.session_id[..8])))
        }),
        role: None,
        parent_pane_id: None,
        can_spawn: None,
        task: None,
        workspace_id: Some(ws_id.clone()),
        force_tab: None,
        report_target_pane_id: None,
        skip_permissions: Some(true),
        spawned_by_pane_id: None,
        model: body.model.clone(),
        resume_session_id: Some(meta.session_id.clone()),
    };
    let spawned = state
        .svc
        .spawn_agent(spawn_params, None)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    state.svc.set_pane_agent_type(&spawned.pane_id, Some(meta.provider.clone()));
    state.svc.emit_state_changed();

    Ok(Json(serde_json::json!({
        "pane_id": spawned.pane_id,
        "tab_id": spawned.tab_id,
        "workspace_id": ws_id,
        "provider": meta.provider,
        "session_id": meta.session_id,
        "cwd": cwd,
        "resume_command": resume_cmd_preview,
    })))
}
