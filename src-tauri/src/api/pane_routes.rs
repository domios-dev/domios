use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, resolve_pane, AppState};

pub async fn list_panes(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let result = crate::dispatch::dispatch(&crate::dispatch::Command::PaneList, &state.svc)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to list panes"))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub struct CreateShellPaneBody {
    pub workspace_id: Option<String>,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub cmd: Option<String>,
    #[serde(default)]
    pub eager: bool,
    /// When set, open a non-PTY chatBackend pane driven by this plugin id
    /// (a `chatBackend`-capability plugin) instead of a shell (P1). `send`/
    /// `get-output` then route to the plugin via the PaneTransport seam.
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

pub async fn create_shell_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateShellPaneBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    use crate::service::spawn::{PaneCommand, PaneVisibility, SpawnPaneSpec};
    // P1: a chatBackend pane is the production entry point for plugin-driven
    // chat. Build PaneCommand::ChatBackend so spawn routes through
    // spawn_chatbackend_impl (no PTY, plugin session, poll loop); otherwise a
    // plain shell pane.
    let command = match body.chat_backend {
        Some(plugin_id) => {
            let mut config = body.config.unwrap_or(serde_json::Value::Null);
            let overrides = crate::service::spawn::chatbackend_spawn_config(
                body.system_prompt.as_deref(),
                body.preset.as_deref(),
                body.model.as_deref(),
            );
            if let (Some(base), Some(extra)) = (config.as_object_mut(), overrides.as_object()) {
                for (key, value) in extra {
                    base.insert(key.clone(), value.clone());
                }
            } else if !overrides.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                config = overrides;
            }
            PaneCommand::ChatBackend { plugin_id, config }
        }
        None => PaneCommand::Shell,
    };
    let handle = state
        .svc
        .spawn_pane(SpawnPaneSpec {
            workspace_id: body.workspace_id,
            cwd: body.cwd,
            title: body.title,
            visibility: PaneVisibility::Visible,
            eager: body.eager,
            command,
            ..Default::default()
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if let Some(cmd) = body.cmd {
        // send_text ensures the PTY itself (ensure_pane_pty_for_send), which a
        // lazy pane needs — a pre-existence poll would never see one appear.
        state
            .svc
            .send_text(&handle.pane_id, &cmd, true)
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!(
                        "pane created (id={}) but --cmd failed: {}",
                        handle.pane_id, e
                    ),
                )
            })?;
    }
    // Mirror `pane_id` as `id` so the conventional CRUD idiom (`jq '.id'`, resp.get("id")) reaches the new pane without anyone having to learn our
    // per-resource naming. Both fields hold the same value.
    Ok(Json(serde_json::json!({
        "id": handle.pane_id,
        "pane_id": handle.pane_id,
        "workspace_id": handle.workspace_id,
        "tab_id": handle.tab_id,
        "pty_id": handle.pty_id,
    })))
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
pub(crate) struct SendTextBody {
    text: Option<String>,
    #[serde(default = "default_true")]
    enter: bool,
    /// Terminal key/action sequence. When present, optional text is inserted
    /// first without auto-submit, then actions are written as raw PTY bytes.
    actions: Option<Vec<SendAction>>,
    /// Batch: send multiple commands sequentially with optional delays
    batch: Option<Vec<BatchCommand>>,
    /// Raw mode: bypass bracket paste, send bytes directly to PTY.
    /// Use for keystrokes that need to be interpreted by TUI (e.g. vim j/k navigation).
    #[serde(default)]
    raw: bool,
    /// Sender's pane id when CLI is invoked from inside CodeTerm. Daemon
    /// prepends a CodeTerm marker so the recipient knows the return-address.
    /// Mesh hops and hierarchy reports use the same marker family.
    #[serde(default)]
    source_pane: Option<String>,
    /// Originating peer name, injected by the mesh relay (authenticated from
    /// the signed token — peers cannot spoof it). Present ⇒ this delivery
    /// crossed the mesh; the label is peer-qualified `label@peer`.
    #[serde(default)]
    source_peer: Option<String>,
    /// Source agent label + model, threaded from the originating peer (the
    /// source pane is foreign and cannot be resolved against the local mux).
    #[serde(default)]
    source_label: Option<String>,
    #[serde(default)]
    source_model: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct BatchCommand {
    text: String,
    #[serde(default = "default_true")]
    enter: bool,
    delay_ms: Option<u64>,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum SendAction {
    Key { key: String },
    Delay { ms: u64 },
}

fn send_action_bytes(key: &str) -> Option<Vec<u8>> {
    let normalized = key.replace('_', "-").to_ascii_lowercase();
    match normalized.as_str() {
        "enter" | "return" => Some(b"\r".to_vec()),
        "esc" | "escape" => Some(b"\x1b".to_vec()),
        "tab" => Some(b"\t".to_vec()),
        "backspace" | "bs" => Some(vec![0x7f]),
        "delete" | "del" => Some(b"\x1b[3~".to_vec()),
        "up" | "arrow-up" => Some(b"\x1b[A".to_vec()),
        "down" | "arrow-down" => Some(b"\x1b[B".to_vec()),
        "right" | "arrow-right" => Some(b"\x1b[C".to_vec()),
        "left" | "arrow-left" => Some(b"\x1b[D".to_vec()),
        "home" => Some(b"\x1b[H".to_vec()),
        "end" => Some(b"\x1b[F".to_vec()),
        "pageup" | "page-up" | "pgup" => Some(b"\x1b[5~".to_vec()),
        "pagedown" | "page-down" | "pgdn" => Some(b"\x1b[6~".to_vec()),
        "space" => Some(b" ".to_vec()),
        other => parse_ctrl_key(other).map(|b| vec![b]),
    }
}

fn parse_ctrl_key(value: &str) -> Option<u8> {
    let key = value
        .strip_prefix("ctrl-")
        .or_else(|| value.strip_prefix("control-"))
        .or_else(|| value.strip_prefix("c-"))?;
    if key.len() != 1 {
        return None;
    }
    let b = key.as_bytes()[0].to_ascii_lowercase();
    if b.is_ascii_lowercase() {
        Some(b - b'a' + 1)
    } else if b == b'[' {
        Some(0x1b)
    } else {
        None
    }
}

fn action_preview(actions: &[SendAction]) -> String {
    actions
        .iter()
        .map(|a| match a {
            SendAction::Key { key } => key.clone(),
            SendAction::Delay { ms } => format!("delay:{ms}ms"),
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// PTY init takes ~1-2s after agent_spawn returns; send fired
/// immediately would otherwise hit "no PTY" unconditionally. Poll for
/// the link with this short budget so a script can chain spawn → send.
async fn await_pty_link(state: &AppState, pane_id: &str) -> bool {
    // [G] A chatBackend pane has no PTY to link — don't poll for one that never
    // appears (it would hang the full budget and then fail). It is "ready" the
    // moment it exists (B1.1#5).
    if state.svc.pane_chat_backing(pane_id).is_some() {
        return true;
    }
    for _ in 0..75 {
        if state.svc.resolve_pty_id(pane_id).is_ok() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    false
}

fn record_push_activity(state: &AppState, svc_id: &Option<String>, pane_id: &str, text: &str) {
    use tauri::Emitter;
    let Some(svc_id) = svc_id else { return };
    let writer = crate::service::activity::ActivityWriter::new(state.svc.services_store.conn());
    let preview = text.chars().take(200).collect::<String>();
    let _ = writer.record(crate::service::activity::ActivityEvent::push(
        svc_id, pane_id, &preview,
    ));
    let _ = state.svc.app_handle.emit(
        "service-pushed",
        serde_json::json!({"source_id": svc_id, "target": pane_id}),
    );
}

pub async fn send_text(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Json(body): Json<SendTextBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    let svc_token_match = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            crate::service::push::validate_push(v, &state.svc.tokens, &state.svc.services_store)
                .ok()
                .flatten()
        });

    if svc_token_match.is_none() {
        check_auth(&state, &headers)?;
    }
    let pane_id = resolve_pane(&state, &raw_pane_id)?;

    state
        .svc
        .ensure_pane_pty_for_send(&pane_id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;

    if !await_pty_link(&state, &pane_id).await {
        return Err((
            StatusCode::NOT_FOUND,
            "Pane not found or PTY did not start within 3s",
        ));
    }

    // Prefix only for agent recipients; shells run the bytes verbatim. One
    // canonical builder serves both local and mesh-relayed deliveries.
    let source_prefix: Option<String> = body.source_pane.as_deref().and_then(|src| {
        let mux = state.svc.mux.lock();
        let target_is_agent = mux
            .get_pane(&pane_id)
            .and_then(|p| p.agent_type.as_ref())
            .is_some();
        match body.source_peer.as_deref().filter(|p| !p.is_empty()) {
            // Mesh delivery: the source pane is foreign — identity comes from
            // the body (resolved on the originating peer), peer-qualified here.
            Some(peer) => {
                let label = body
                    .source_label
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(peer);
                crate::service::msg_prefix::delivery_prefix(
                    target_is_agent,
                    src,
                    &pane_id,
                    label,
                    body.source_model.as_deref(),
                    Some(peer),
                )
            }
            // Local delivery: resolve the label/model from the local mux.
            None => {
                let from_label = mux.display_label_for_pane(src);
                let model = mux
                    .get_pane(src)
                    .and_then(|p| p.agent_session_model.clone());
                crate::service::msg_prefix::delivery_prefix(
                    target_is_agent,
                    src,
                    &pane_id,
                    &from_label,
                    model.as_deref(),
                    None,
                )
            }
        }
    });
    let prepend = |s: &str| -> String {
        match &source_prefix {
            Some(p) => format!("{}{}", p, s),
            None => s.to_string(),
        }
    };

    // P3/F3: run the outbound text stages (chatPreprocessor) for a USER turn to a
    // PTY agent pane. Service pushes (svc_token) and inter-pane/mesh/report
    // deliveries (source_pane) are system traffic and excluded; chatBackend panes
    // already stage at their transport, so they're excluded here to avoid
    // double-processing. System senders (orchestrator/restart/GA) never reach this
    // REST seam — they use the synchronous `send_text`.
    let run_stages = crate::chatpipeline::is_user_turn_send(
        svc_token_match.is_some(),
        body.source_pane.is_some(),
        state.svc.pane_chat_backing(&pane_id).is_some(),
        state.svc.pane_has_agent(&pane_id),
    );
    let stage = |s: String| -> String {
        if run_stages {
            state.svc.run_outbound_stages(&pane_id, s)
        } else {
            s
        }
    };

    if let Some(actions) = body.actions {
        if actions.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "actions cannot be empty"));
        }
        let mut writes: Vec<(Option<u64>, Vec<u8>)> = Vec::with_capacity(actions.len());
        for action in &actions {
            match action {
                SendAction::Delay { ms } => writes.push((Some(*ms), Vec::new())),
                SendAction::Key { key } => {
                    let Some(bytes) = send_action_bytes(key) else {
                        return Err((StatusCode::BAD_REQUEST, "unknown action key"));
                    };
                    writes.push((None, bytes));
                }
            }
        }

        let raw_text = body.text.clone().unwrap_or_default();
        let text_was_sent = !raw_text.is_empty();
        if text_was_sent {
            let text = prepend(&stage(raw_text.clone()));
            state
                .svc
                .send_text_when_ready(&pane_id, &text, false)
                .await
                .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;
        }

        // Raw terminal key writes are PTY-only; a chatBackend pane has no
        // terminal, so skip them (the text turn above already routed to the
        // plugin via send_text_when_ready).
        if state.svc.pane_chat_backing(&pane_id).is_none() {
            let pty_id = state
                .svc
                .resolve_pty_id(&pane_id)
                .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;
            let pty = state.svc.pty.clone();
            tokio::spawn(async move {
                if text_was_sent {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                for (delay, bytes) in writes {
                    if let Some(ms) = delay {
                        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                        continue;
                    }
                    let _ = pty.lock().write(&pty_id, &bytes);
                }
            });
        }

        let preview = if raw_text.is_empty() {
            format!("<actions:{}>", action_preview(&actions))
        } else {
            format!("{} <actions:{}>", raw_text, action_preview(&actions))
        };
        record_push_activity(&state, &svc_token_match, &pane_id, &preview);
        return Ok(Json(serde_json::json!({"ok": true, "actions": true})));
    }

    if let Some(batch) = body.batch {
        let preview = batch
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join(" ; ");
        let items: Vec<crate::service::pty_io::SendBatchItem> = batch
            .into_iter()
            .map(|c| crate::service::pty_io::SendBatchItem {
                text: prepend(&stage(c.text)),
                enter: c.enter,
                delay_ms: c.delay_ms,
            })
            .collect();
        state
            .svc
            .send_text_batch(&pane_id, items)
            .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;
        record_push_activity(&state, &svc_token_match, &pane_id, &preview);
        return Ok(Json(serde_json::json!({"ok": true, "batch": true})));
    }

    if body.raw {
        // raw mode bypasses bracket paste — sender prefix is intentionally skipped here too (raw = byte-for-byte keystrokes for TUIs)
        let raw_text = body.text.clone().unwrap_or_default();
        let mut buf = raw_text.clone();
        if body.enter && !buf.ends_with('\r') {
            buf.push('\r');
        }
        state
            .svc
            .write_raw(&pane_id, buf.as_bytes())
            .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;
        record_push_activity(&state, &svc_token_match, &pane_id, &raw_text);
        return Ok(Json(serde_json::json!({"ok": true})));
    }

    let text = prepend(&stage(body.text.unwrap_or_default()));
    state
        .svc
        .send_text_when_ready(&pane_id, &text, body.enter)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;
    record_push_activity(&state, &svc_token_match, &pane_id, &text);
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Interrupt the agent's current operation (NOT a clean exit).
/// Reads `provider.stop_keystroke()` for the pane's agent_type and
/// writes those raw bytes to the PTY. 200 + `{stopped:false}` when
/// the provider has no stop keystroke declared; 404 when the pane
/// or PTY is gone. Used by the per-pane Stop button.
pub async fn stop_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let agent_id = state
        .svc
        .mux
        .lock()
        .get_pane(&pane_id)
        .and_then(|p| p.agent_type.clone());
    let bytes: Option<String> = agent_id.and_then(|aid| {
        state
            .svc
            .agent_registry
            .with_provider(&aid, |p| p.stop_keystroke().map(|s| s.to_string()))
            .flatten()
    });
    let Some(bytes) = bytes else {
        return Ok(Json(serde_json::json!({"ok": true, "stopped": false})));
    };
    state
        .svc
        .write_raw(&pane_id, bytes.as_bytes())
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;
    Ok(Json(serde_json::json!({"ok": true, "stopped": true})))
}

pub async fn focus_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let result =
        crate::dispatch::dispatch(&crate::dispatch::Command::PaneFocus { pane_id }, &state.svc)
            .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found"))?;
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

pub async fn get_pane_output(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Query(query): Query<OutputQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let cmd = crate::dispatch::Command::GetOutput {
        pane_id,
        offset: query.offset,
        max_bytes: None,
        strip_ansi: query.strip_ansi.unwrap_or(false),
        full: query.full.unwrap_or(false),
    };
    let result = crate::dispatch::dispatch(&cmd, &state.svc)
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub(crate) struct ExecBody {
    command: String,
    timeout_ms: Option<u64>,
    strip_ansi: Option<bool>,
}

pub async fn exec_in_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Json(body): Json<ExecBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;

    if !await_pty_link(&state, &pane_id).await {
        return Err((
            StatusCode::NOT_FOUND,
            "Pane not found or PTY did not start within 3s",
        ));
    }
    let pty_id = state
        .svc
        .resolve_pty_id(&pane_id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found or has no PTY"))?;

    let start_offset = state
        .svc
        .output_buf
        .lock()
        .get_text(&pty_id)
        .map(|t| t.len())
        .unwrap_or(0);

    state
        .svc
        .send_text(&pane_id, &body.command, true)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to write to PTY"))?;

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
        crate::pty::ansi::clean_terminal_output(&raw_output)
    } else {
        raw_output
    };
    let mut resp = serde_json::json!({"output": output, "offset": offset});
    if timed_out {
        resp["timeout"] = serde_json::json!(true);
    }
    Ok(Json(resp))
}

pub async fn close_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let result =
        crate::dispatch::dispatch(&crate::dispatch::Command::PaneClose { pane_id }, &state.svc)
            .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found"))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub(crate) struct RenamePaneBody {
    title: String,
}

pub async fn rename_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Json(body): Json<RenamePaneBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let result = crate::dispatch::dispatch(
        &crate::dispatch::Command::PaneRename {
            pane_id,
            title: body.title,
        },
        &state.svc,
    )
    .map_err(|_| (StatusCode::NOT_FOUND, "Pane not found"))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub(crate) struct ResizePaneBody {
    cols: u16,
    rows: u16,
}

pub async fn resize_pane(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Json(body): Json<ResizePaneBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let result = crate::dispatch::dispatch(
        &crate::dispatch::Command::PtyResize {
            pane_id,
            cols: body.cols,
            rows: body.rows,
        },
        &state.svc,
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

pub async fn stream_pane_output(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Query(query): Query<StreamQuery>,
) -> Result<
    axum::response::Sse<
        impl futures_core::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
    >,
    (StatusCode, &'static str),
> {
    check_auth(&state, &headers)?;
    // A chatBackend pane has no PTY output buffer — it streams transcript via the
    // `pane-chat-msg-{id}` Tauri event (StructuredSink), not this SSE. Keep the
    // connection alive but emit nothing rather than erroring "no PTY" (B1.2 [T]).
    let chat = state.svc.pane_chat_backing(&raw_pane_id).is_some();
    let pty_id = if chat {
        String::new()
    } else {
        resolve_pty_id_with_fallback(&state, &raw_pane_id)?
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
    Path(raw_pane_id): Path<String>,
) -> Result<
    (
        StatusCode,
        [(axum::http::header::HeaderName, &'static str); 1],
        String,
    ),
    (StatusCode, &'static str),
> {
    check_auth(&state, &headers)?;

    if let Ok(pty_id) = resolve_pty_id_with_fallback(&state, &raw_pane_id) {
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

    if let Some(text) = read_log_from_disk_by_pane(&state, &raw_pane_id) {
        return Ok((
            StatusCode::OK,
            [(
                axum::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8",
            )],
            text,
        ));
    }

    Err((StatusCode::NOT_FOUND, "Pane not found or has no PTY"))
}

fn read_log_from_disk_by_pane(state: &AppState, raw_pane_id: &str) -> Option<String> {
    for worker in state.svc.workers.list() {
        if worker.pane_id == raw_pane_id {
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
            if run.pane_id.as_deref() == Some(raw_pane_id) {
                if let Some(text) = store.read_log(&job_id, &run.run_id) {
                    return Some(text);
                }
            }
        }
    }
    None
}

// close_pane can remove pane→pty, but output_buf[pty_id] may remain.
// Fall back to spawn-time map so worker/cron logs from closed panes stay readable.
fn resolve_pty_id_with_fallback(
    state: &AppState,
    raw_pane_id: &str,
) -> Result<String, (StatusCode, &'static str)> {
    if let Ok(pane_id) = resolve_pane(state, raw_pane_id) {
        if let Ok(pty_id) = state.svc.resolve_pty_id(&pane_id) {
            return Ok(pty_id);
        }
        if let Some(pty_id) = state.svc.agent_run_pty_map.lock().get(&pane_id).cloned() {
            return Ok(pty_id);
        }
    }
    if let Some(pty_id) = state.svc.agent_run_pty_map.lock().get(raw_pane_id).cloned() {
        return Ok(pty_id);
    }
    Err((StatusCode::NOT_FOUND, "Pane not found or has no PTY"))
}

#[derive(Deserialize, Default)]
pub(crate) struct GetTextQuery {
    #[serde(default)]
    formatted: bool,
}

pub async fn get_pane_usage(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;

    let (agent_type, session_id, cwd) = {
        let mux = state.svc.mux.lock();
        let pane = mux
            .get_pane(&pane_id)
            .ok_or((StatusCode::NOT_FOUND, "Pane not found"))?;
        (
            pane.agent_type.clone(),
            pane.agent_session_id.clone(),
            pane.cwd.clone(),
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

pub async fn get_pane_text(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(raw_pane_id): Path<String>,
    Query(query): Query<GetTextQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let pane_id = resolve_pane(&state, &raw_pane_id)?;
    let cmd = crate::dispatch::Command::GetText {
        pane_id: pane_id.clone(),
        formatted: query.formatted,
    };
    match crate::dispatch::dispatch(&cmd, &state.svc) {
        Ok(r) => {
            let mut v = r.into_json();
            if let Some(obj) = v.as_object_mut() {
                obj.insert("pane_id".to_string(), serde_json::json!(pane_id));
            }
            Ok(Json(v))
        }
        Err(_) => Err((StatusCode::NOT_FOUND, "Pane not found or has no PTY")),
    }
}

#[cfg(test)]
mod tests_send_text_body;
