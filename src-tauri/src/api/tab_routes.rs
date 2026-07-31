use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, resolve_tab, AppState};
use send::wait_for_pty_link as await_pty_link;

mod send;

pub use send::{chat_send_status, chat_send_text, send_text};

#[cfg(test)]
pub(crate) use send::{
    send_action_bytes, should_prepend_source, BatchCommand, SendAction, SendTextBody,
    TabInputTransport,
};

pub async fn list_tabs(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(query): Query<TabListQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let focused_workspace =
        !query.all_workspaces && (query.focused_workspace || query.workspace_id.is_none());
    let caller_tab_id = if query.all_workspaces {
        None
    } else {
        caller_tab_header(&headers)
    };
    let result = crate::dispatch::dispatch(
        &crate::dispatch::Command::TabSummary {
            workspace_id: query.workspace_id,
            caller_tab_id,
            focused_workspace,
            agents_only: query.agents,
        },
        state.svc.as_ref(),
    )
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to list Tabs"))?;
    Ok(Json(result.into_json()))
}

#[derive(Default, Deserialize)]
pub struct TabListQuery {
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub focused_workspace: bool,
    #[serde(default)]
    pub all_workspaces: bool,
    #[serde(default)]
    pub agents: bool,
}

#[derive(Deserialize)]
pub struct TabInspectQuery {
    pub tab_ids: String,
}

pub async fn inspect_tabs(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(query): Query<TabInspectQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let tab_ids = query
        .tab_ids
        .split(',')
        .filter(|tab_id| !tab_id.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if tab_ids.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "tab_ids is required"));
    }
    let result = crate::dispatch::dispatch(
        &crate::dispatch::Command::TabInspect { tab_ids },
        state.svc.as_ref(),
    )
    .map_err(|_| (StatusCode::NOT_FOUND, "Requested Tab was not found"))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub struct CreateShellTabBody {
    pub workspace_id: Option<String>,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub cmd: Option<String>,
    #[serde(default)]
    pub eager: bool,
    /// When set, open a non-PTY chatBackend tab driven by this plugin id
    /// (a `chatBackend`-capability plugin) instead of a shell (P1). `send`/
    /// `get-output` then route to the plugin via the TabTransport seam.
    #[serde(default)]
    pub chat_backend: Option<String>,
    /// Config object handed to the chatBackend plugin's `openSession` — only
    /// meaningful with `chat_backend`.
    #[serde(default)]
    pub config: Option<serde_json::Value>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

pub async fn create_shell_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateShellTabBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    use crate::service::spawn::{SpawnTabSpec, TabCommand, TabVisibility};
    let command = match body.chat_backend {
        Some(plugin_id) => {
            validate_chat_backend_spawn(&state.svc.provider_registry, &plugin_id)
                .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
            let mut config = body.config.unwrap_or(serde_json::Value::Null);
            let overrides = crate::service::spawn::chatbackend_spawn_config(
                body.system_prompt.as_deref(),
                body.preset.as_deref(),
                body.model.as_deref(),
                None,
                None,
                None,
            );
            if let (Some(base), Some(extra)) = (config.as_object_mut(), overrides.as_object()) {
                for (key, value) in extra {
                    base.insert(key.clone(), value.clone());
                }
            } else if !overrides.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                config = overrides;
            }
            crate::service::spawn::reject_direct_watcher_chatbackend_config(&config)
                .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
            TabCommand::ChatBackend {
                plugin_id,
                config,
                role: None,
                role_profile: None,
                can_spawn: None,
                watcher: None,
            }
        }
        None => TabCommand::Shell,
    };
    let handle = state
        .svc
        .spawn_tab(SpawnTabSpec {
            workspace_id: body.workspace_id,
            cwd: body.cwd,
            title: body.title,
            description: body
                .description
                .map(|s| crate::service::normalize_tab_description(&s))
                .transpose()
                .map_err(|e| (StatusCode::BAD_REQUEST, e))?
                .flatten(),
            visibility: TabVisibility::Visible,
            eager: body.eager,
            command,
            ..Default::default()
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if let Some(cmd) = body.cmd {
        // bare shells which haven't armed DECSET ?2004 yet.
        state
            .svc
            .send_shell_cmd(&handle.tab_id, &cmd)
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Tab created (id={}) but --cmd failed: {}", handle.tab_id, e),
                )
            })?;
    }
    // per-resource naming. Both fields hold the same value.
    let mut resp = serde_json::json!({
        "id": handle.tab_id,
        "tab_id": handle.tab_id,
        "workspace_id": handle.workspace_id,
        "tab_id": handle.tab_id,
        "pty_id": handle.pty_id,
    });
    if let Some(ref notice) = handle.budget_notice {
        resp["budget_notice"] = serde_json::to_value(notice).unwrap_or_default();
    }
    Ok(Json(resp))
}

/// Validate a `chat_backend` spawn target for POST /api/tabs. `Ok(())` ⇒ the id
/// names a registered, user-spawnable chat backend; `Err(reason)` ⇒ a 400 the
/// caller surfaces (unknown id vs. registered-but-not-spawnable, distinguished
/// so the message is actionable).
fn validate_chat_backend_spawn(
    reg: &crate::provider::ProviderRegistry,
    plugin_id: &str,
) -> Result<(), String> {
    if reg.is_spawnable_chat_backend(plugin_id) {
        Ok(())
    } else if reg.transport_of(plugin_id).is_some() {
        Err(format!(
            "chat backend '{plugin_id}' is registered but not user-spawnable"
        ))
    } else {
        Err(format!("unknown chat backend '{plugin_id}'"))
    }
}

/// Interrupt the agent's current operation (NOT a clean exit).
/// Reads `provider.stop_keystroke()` for the tab's agent_type and
/// writes those raw bytes to the PTY. 200 + `{stopped:false}` when
/// the provider has no stop keystroke declared; 404 when the tab
/// or PTY is gone. Used by the per-tab Stop button.
pub async fn stop_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let tab_id = resolve_tab(&state, &raw_tab_id)?;
    let (agent_id, session_id) = {
        let mux = state.svc.mux.write();
        match mux.get_tab(&tab_id) {
            Some(tab) => (tab.agent_type.clone(), tab.agent_session_id.clone()),
            None => (None, None),
        }
    };
    // No agent on this tab → nothing to stop.
    let Some(aid) = agent_id else {
        return Ok(Json(serde_json::json!({"ok": true, "stopped": false})));
    };
    if let Some(sid) = session_id {
        let svc = state.svc.clone();
        let tab = tab_id.clone();
        let structured_abort = tokio::task::spawn_blocking(move || {
            crate::service::structured_prompt::abort_session(&svc, &tab, &sid)
        })
        .await
        .ok()
        .flatten();
        if let Some(Ok(())) = structured_abort {
            if let Ok(pty_id) = resolve_pty_id_with_fallback(&state, &raw_tab_id) {
                state.svc.mark_agent_interrupted(&pty_id);
            }
            return Ok(Json(
                serde_json::json!({"ok": true, "stopped": true, "structured_abort": true}),
            ));
        }
    }
    let key: String = state
        .svc
        .agent_registry
        .with_provider(&aid, |p| p.stop_keystroke().map(|s| s.to_string()))
        .flatten()
        .unwrap_or_else(|| "\x1b".to_string());
    if let Ok(pty_id) = resolve_pty_id_with_fallback(&state, &raw_tab_id) {
        state.svc.mark_agent_interrupted(&pty_id);
    }
    state
        .svc
        .write_raw(&tab_id, key.as_bytes())
        .map_err(|_| (StatusCode::NOT_FOUND, "Tab not found or has no PTY"))?;
    Ok(Json(serde_json::json!({"ok": true, "stopped": true})))
}

pub async fn focus_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let tab_id = resolve_tab(&state, &raw_tab_id)?;
    let result = crate::dispatch::dispatch(
        &crate::dispatch::Command::TabFocus { tab_id },
        state.svc.as_ref(),
    )
    .map_err(|_| (StatusCode::NOT_FOUND, "Tab not found"))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub(crate) struct OutputQuery {
    offset: Option<usize>,
    strip_ansi: Option<bool>,
    #[allow(dead_code)]
    screen: Option<bool>,
    full: Option<bool>,
}

pub async fn get_tab_output(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    Query(query): Query<OutputQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let tab_id = resolve_tab(&state, &raw_tab_id)?;
    let cmd = crate::dispatch::Command::GetOutput {
        tab_id,
        offset: query.offset,
        max_bytes: None,
        strip_ansi: query.strip_ansi.unwrap_or(false),
        full: query.full.unwrap_or(false),
    };
    let result = crate::dispatch::dispatch_blocking(cmd, state.svc.clone())
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, "Tab not found or has no PTY"))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub(crate) struct ExecBody {
    command: String,
    timeout_ms: Option<u64>,
    strip_ansi: Option<bool>,
}

pub async fn exec_in_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    Json(body): Json<ExecBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let tab_id = resolve_tab(&state, &raw_tab_id)?;

    // the link, else await_pty_link waits for a PTY nothing triggered and fails.
    state
        .svc
        .ensure_tab_pty_for_send(&tab_id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Tab not found or has no PTY"))?;
    if !await_pty_link(&state, &tab_id).await {
        return Err((StatusCode::NOT_FOUND, "Tab not found or PTY did not start"));
    }
    let pty_id = state
        .svc
        .resolve_pty_id(&tab_id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Tab not found or has no PTY"))?;

    let start_offset = state
        .svc
        .output_buf
        .lock()
        .get_text(&pty_id)
        .map(|t| t.len())
        .unwrap_or(0);

    // path; plain shell tabs use script-exec so markers never leak literally.
    let has_agent = state.svc.tab_has_agent(&tab_id);
    if has_agent {
        state
            .svc
            .send_text(&tab_id, &body.command)
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to write to PTY"))?;
    } else {
        state
            .svc
            .send_shell_spawn(&pty_id, &body.command)
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to write to PTY"))?;
    }

    let timeout_ms = body.timeout_ms.unwrap_or(10000);
    let buf_clone = state.svc.output_buf.clone();
    let pty_clone = pty_id.clone();

    let result = tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
            let len1 = buf_clone
                .lock()
                .get_text(&pty_clone)
                .map(|t| t.len())
                .unwrap_or(0);
            if len1 > start_offset {
                tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
                let len2 = buf_clone
                    .lock()
                    .get_text(&pty_clone)
                    .map(|t| t.len())
                    .unwrap_or(0);
                if len2 == len1 || tokio::time::Instant::now() >= deadline {
                    let text = buf_clone.lock().get_text(&pty_clone).unwrap_or_default();
                    let new = if start_offset < text.len() {
                        text[start_offset..].to_string()
                    } else {
                        String::new()
                    };
                    return (new, text.len(), false);
                }
            }
            if tokio::time::Instant::now() >= deadline {
                let text = buf_clone.lock().get_text(&pty_clone).unwrap_or_default();
                let new = if start_offset < text.len() {
                    text[start_offset..].to_string()
                } else {
                    String::new()
                };
                return (new, text.len(), true);
            }
        }
    })
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Exec task failed"))?;

    let (raw_output, offset, timed_out) = result;
    let output = if body.strip_ansi.unwrap_or(false) {
        // Re-emulate at the pty's REAL width: shell partial-line redraws
        // (zsh's % padding) erase the last output line at any other width.
        let cols = state
            .svc
            .pty
            .lock()
            .size(&pty_id)
            .map(|(c, _)| c)
            .unwrap_or(200);
        crate::pty::ansi::clean_terminal_output_sized(&raw_output, 500, cols)
    } else {
        raw_output
    };
    let mut resp = serde_json::json!({"output": output, "offset": offset});
    if timed_out {
        resp["timeout"] = serde_json::json!(true);
    }
    Ok(Json(resp))
}

pub async fn close_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let tab_id = resolve_tab(&state, &raw_tab_id).map_err(|(s, m)| (s, m.to_string()))?;
    if let Some(caller_tab_id) = caller_tab_header(&headers) {
        let result = crate::dispatch::dispatch(
            &crate::dispatch::Command::TabClose {
                tab_id,
                caller_tab_id: Some(caller_tab_id),
            },
            state.svc.as_ref(),
        )
        .map_err(map_close_error)?;
        return Ok(Json(result.into_json()));
    }
    state.svc.close_tab(&tab_id).map_err(map_close_error)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub(crate) fn caller_tab_header(headers: &HeaderMap) -> Option<String> {
    headers
        .get("X-CodeTerm-Tab-Id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// A self-termination refusal is a 403 (authenticated but forbidden); anything
/// else from a close is a missing target.
pub(crate) fn map_close_error(e: String) -> (StatusCode, String) {
    if e.starts_with(crate::service::tab::SELF_CLOSE_REFUSAL_PREFIX) {
        (StatusCode::FORBIDDEN, e)
    } else {
        (StatusCode::NOT_FOUND, e)
    }
}

#[derive(Deserialize)]
pub(crate) struct RenameTabBody {
    title: String,
}

pub async fn rename_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    Json(body): Json<RenameTabBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let tab_id = resolve_tab(&state, &raw_tab_id)?;
    let result = crate::dispatch::dispatch(
        &crate::dispatch::Command::TabRename {
            tab_id,
            title: body.title,
        },
        state.svc.as_ref(),
    )
    .map_err(|_| (StatusCode::NOT_FOUND, "Tab not found"))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub(crate) struct DescribeTabBody {
    description: String,
}

pub async fn describe_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    Json(body): Json<DescribeTabBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    let tab_id = resolve_tab(&state, &raw_tab_id).map_err(|(s, m)| (s, m.to_string()))?;
    let result = crate::dispatch::dispatch(
        &crate::dispatch::Command::TabDescribe {
            tab_id,
            description: body.description,
        },
        state.svc.as_ref(),
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub(crate) struct ResizeTabBody {
    cols: u16,
    rows: u16,
}

pub async fn resize_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    Json(body): Json<ResizeTabBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let tab_id = resolve_tab(&state, &raw_tab_id)?;
    let result = crate::dispatch::dispatch(
        &crate::dispatch::Command::PtyResize {
            tab_id,
            cols: body.cols,
            rows: body.rows,
        },
        state.svc.as_ref(),
    )
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to resize PTY"))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub(crate) struct StreamQuery {
    /// Absolute byte offset (monotonic, survives ring-buffer rotation).
    /// `0` replays the full retained buffer; omitted = start at current tail.
    from: Option<u64>,
}

pub async fn stream_tab_output(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    Query(query): Query<StreamQuery>,
) -> Result<
    axum::response::Sse<
        impl futures_core::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
    >,
    (StatusCode, &'static str),
> {
    check_auth(&state, &headers)?;
    let chat = state.svc.tab_chat_backing(&raw_tab_id).is_some();
    let pty_id = if chat {
        String::new()
    } else {
        resolve_pty_id_with_fallback(&state, &raw_tab_id)?
    };

    let buf = state.svc.output_buf.clone();
    let pid = pty_id.clone();
    let initial_total = if chat {
        0
    } else {
        buf.lock().total_written(&pid)
    };
    let mut offset: u64 = match query.from {
        Some(n) => n.min(initial_total),
        None => initial_total,
    };

    let stream = async_stream::stream! {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            if chat {
                continue;
            }
            let (chunk, new_total) = buf.lock().read_since(&pid, offset);
            if !chunk.is_empty() {
                offset = new_total;
                let text = String::from_utf8_lossy(&chunk).to_string();
                yield Ok(axum::response::sse::Event::default()
                    .data(serde_json::json!({"output": text, "offset": offset}).to_string()));
            }
        }
    };

    Ok(axum::response::Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default()))
}

pub async fn agent_run_log(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
) -> Result<
    (
        StatusCode,
        [(axum::http::header::HeaderName, &'static str); 1],
        String,
    ),
    (StatusCode, &'static str),
> {
    check_auth(&state, &headers)?;

    if let Ok(pty_id) = resolve_pty_id_with_fallback(&state, &raw_tab_id) {
        let text = state
            .svc
            .output_buf
            .lock()
            .get_full_text(&pty_id)
            .unwrap_or_default();
        if !text.is_empty() {
            return Ok((
                StatusCode::OK,
                [(
                    axum::http::header::CONTENT_TYPE,
                    "text/plain; charset=utf-8",
                )],
                text,
            ));
        }
    }

    if let Some(text) = read_log_from_disk_by_tab(&state, &raw_tab_id) {
        return Ok((
            StatusCode::OK,
            [(
                axum::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8",
            )],
            text,
        ));
    }

    Err((StatusCode::NOT_FOUND, "Tab not found or has no PTY"))
}

fn read_log_from_disk_by_tab(state: &AppState, raw_tab_id: &str) -> Option<String> {
    for worker in state.svc.workers.list() {
        if worker.tab_id == raw_tab_id {
            if let Some(ref path) = worker.log_path {
                if let Ok(text) = std::fs::read_to_string(path) {
                    return Some(text);
                }
            }
            break;
        }
    }
    let store = state.cron.lock();
    let jobs: Vec<String> = store.list().iter().map(|j| j.id.clone()).collect();
    for job_id in jobs {
        for run in store.get_history(&job_id) {
            if run.tab_id.as_deref() == Some(raw_tab_id) {
                if let Some(text) = store.read_log(&job_id, &run.run_id) {
                    return Some(text);
                }
            }
        }
    }
    None
}

// close_tab can remove tab→pty, but output_buf[pty_id] may remain.
// Fall back to spawn-time map so worker/cron logs from closed tabs stay readable.
fn resolve_pty_id_with_fallback(
    state: &AppState,
    raw_tab_id: &str,
) -> Result<String, (StatusCode, &'static str)> {
    if let Ok(tab_id) = resolve_tab(state, raw_tab_id) {
        if let Ok(pty_id) = state.svc.resolve_pty_id(&tab_id) {
            return Ok(pty_id);
        }
        if let Some(pty_id) = state.svc.agent_run_pty_map.lock().get(&tab_id).cloned() {
            return Ok(pty_id);
        }
    }
    if let Some(pty_id) = state.svc.agent_run_pty_map.lock().get(raw_tab_id).cloned() {
        return Ok(pty_id);
    }
    Err((StatusCode::NOT_FOUND, "Tab not found or has no PTY"))
}

#[derive(Deserialize, Default)]
pub(crate) struct GetTextQuery {
    #[serde(default)]
    formatted: bool,
    #[serde(default)]
    caller: Option<String>,
}

pub async fn get_tab_usage(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let tab_id = resolve_tab(&state, &raw_tab_id)?;

    let (agent_type, session_id, cwd) = {
        let mux = state.svc.mux.write();
        let tab = mux
            .get_tab(&tab_id)
            .ok_or((StatusCode::NOT_FOUND, "Tab not found"))?;
        (
            tab.agent_type.clone(),
            tab.agent_session_id.clone(),
            tab.cwd.clone(),
        )
    };

    let (Some(agent_type), Some(session_id), Some(cwd)) = (agent_type, session_id, cwd) else {
        return Ok(Json(
            serde_json::json!({ "usage": serde_json::Value::Null }),
        ));
    };

    let usage = state
        .svc
        .agent_registry
        .with_provider(&agent_type, |p| p.read_usage(&cwd, &session_id))
        .flatten();

    Ok(Json(serde_json::json!({ "usage": usage })))
}

pub async fn get_tab_text(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    Query(query): Query<GetTextQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let tab_id = resolve_tab(&state, &raw_tab_id)?;
    if let Some(caller) = query.caller.as_deref().filter(|s| !s.is_empty()) {
        let caller = resolve_tab(&state, caller).unwrap_or_else(|_| caller.to_string());
        state.svc.maybe_send_polling_nudge(&caller, &tab_id);
    }
    let cmd = crate::dispatch::Command::GetText {
        tab_id: tab_id.clone(),
        formatted: query.formatted,
    };
    match crate::dispatch::dispatch_blocking(cmd, state.svc.clone()).await {
        Ok(r) => {
            let mut v = r.into_json();
            if let Some(obj) = v.as_object_mut() {
                obj.insert("tab_id".to_string(), serde_json::json!(tab_id));
            }
            Ok(Json(v))
        }
        Err(_) => Err((StatusCode::NOT_FOUND, "Tab not found or has no PTY")),
    }
}

/// GET /api/tabs/{id}/shell-commands — OSC4 chat snapshot hydration: the
/// tab's accumulated shell command list + cwd. Empty (not 404) when the tab
/// has no PTY or no shell-integration markers yet, so chat mount never errors.
pub async fn shell_command_snapshot(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pty_id = resolve_pty_id_with_fallback(&state, &raw_tab_id).ok();
    let snap = pty_id.and_then(|pty_id| state.svc.shell_caps.lock().snapshot(&pty_id));
    let snap = snap.unwrap_or_default();
    serde_json::to_value(snap)
        .map(Json)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "serialize failed"))
}

#[derive(Deserialize)]
pub struct CaptureStartBody {
    /// Path of the `.cast` file to write.
    pub out: String,
}

/// POST /api/tabs/{id}/capture/start — dev-only PTY capture tap → asciicast v2.
pub async fn start_capture(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
    Json(body): Json<CaptureStartBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pty_id = resolve_pty_id_with_fallback(&state, &raw_tab_id)?;
    let (cols, rows) = {
        let mgr = state.svc.pty.lock();
        match mgr.get_session(&pty_id) {
            Some(s) => *s
                .size
                .lock()
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "pty size lock poisoned"))?,
            None => (80, 24),
        }
    };
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let out = std::path::PathBuf::from(&body.out);
    let prefill = state
        .svc
        .output_buf
        .lock()
        .get_full(&pty_id)
        .map(|bytes| bytes.to_vec())
        .unwrap_or_default();
    crate::pty::capture::CaptureRegistry::global()
        .start(&pty_id, out, cols, rows, timestamp, prefill)
        .map_err(|_| {
            (
                StatusCode::CONFLICT,
                "Failed to start capture (already active?)",
            )
        })?;
    Ok(Json(serde_json::json!({
        "ok": true, "out": body.out, "width": cols, "height": rows,
    })))
}

/// POST /api/tabs/{id}/capture/stop — flush+close `.cast`, write sidecar screen.txt.
pub async fn stop_capture(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_tab_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pty_id = resolve_pty_id_with_fallback(&state, &raw_tab_id)?;
    // Render the final grid from the SAME vt100 ScreenState the daemon uses.
    let screen = state.svc.screen_state.lock().get_screen_text(&pty_id);
    let reg = crate::pty::capture::CaptureRegistry::global();
    // stop() joins the writer thread (file IO) — keep it off the async runtime.
    let pty_for_stop = pty_id.clone();
    let out = tokio::task::spawn_blocking(move || reg.stop(&pty_for_stop))
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "capture stop join failed",
            )
        })?
        .ok_or((StatusCode::NOT_FOUND, "no active capture for this Tab"))?;
    let screen_path = out.with_extension("screen.txt");
    if let Some(text) = screen {
        let _ = std::fs::write(&screen_path, text);
    }
    Ok(Json(serde_json::json!({
        "ok": true,
        "cast": out.to_string_lossy(),
        "screen": screen_path.to_string_lossy(),
    })))
}

#[derive(Deserialize)]
pub(crate) struct LinkPtyBody {
    pty_id: String,
}

/// Web-mirror twin of `mux_link_pty` — bind a spawned PTY to its tab record.
pub async fn link_pty(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(tab_id): Path<String>,
    Json(body): Json<LinkPtyBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let selected_pty_id = state.svc.link_pty(&tab_id, body.pty_id);
    Ok(Json(
        serde_json::json!({"ok": true, "pty_id": selected_pty_id}),
    ))
}

#[cfg(test)]
mod tests_send_text_body;

#[cfg(test)]
mod tests_create_tab;
