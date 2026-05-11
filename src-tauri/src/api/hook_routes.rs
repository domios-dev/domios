use axum::{
    extract::{Path, State as AxumState},
    http::StatusCode,
    Json,
};

use crate::api::routes::{resolve_pane, AppState};

/// Ensure agent state is set when a hook fires.
/// Hooks are authoritative — if a hook calls us, an agent IS running.
/// This fixes detection on restored panes, Windows (PowerShell doesn't reset title), etc.
fn ensure_agent_state(state: &AppState, pane_id: &str) {
    log::info!("[ensure_agent_state] called for pane {}", pane_id);
    let (agent_type, session_id, cwd) = {
        let mux = state.svc.mux.lock();
        let pane = match mux.get_pane(pane_id) {
            Some(p) => p,
            None => return,
        };
        (pane.agent_type.clone(), pane.agent_session_id.clone(), pane.cwd.clone())
    };

    let agent_changed = agent_type.is_none();
    if agent_changed {
        // TODO(plugin-prep): when plugins can register HTTP routes, move this whole hook
        // handler into plugin/agent_claude/ and let each provider own its own hook endpoints.
        state.svc.set_and_lock_pane_agent_type(pane_id, "claude".to_string());
        log::info!("[hook] set agent_type=claude for pane {}", pane_id);
    }

    // Hook proves agent is alive, so use a generous 24-hour recency window.
    if let Some(ref cwd) = cwd {
        let exclude = state.svc.mux.lock().other_session_ids(pane_id);
        let hook_max_age = Some(std::time::Duration::from_secs(86400));
        log::info!("[ensure_agent_state] detecting session cwd={} current={:?}", cwd, session_id);
        if let Some(newest_id) = state.svc.agent_registry
            .with_provider("claude", |p| p.detect_session_id(cwd, &exclude, hook_max_age))
            .flatten()
        {
            let changed = session_id.as_deref() != Some(newest_id.as_str());
            log::info!("[ensure_agent_state] newest={} changed={}", newest_id, changed);
            if changed {
                state.svc.set_agent_session_id(pane_id, Some(newest_id.clone()));
                log::info!("[hook] session updated: {} -> {} for pane {}",
                    session_id.as_deref().unwrap_or("none"), newest_id, pane_id);
                state.svc.emit_state_changed();
            }
        }
    }

    if agent_changed {
        state.svc.emit_state_changed();
    }
}

/// Hook endpoint: agent reports it needs permission.
/// Called by Claude Code hook (permission_prompt matcher).
/// No auth required — hooks run in the PTY process context.
///
/// Accepts optional JSON body with explicit prompt data:
///   { "question": "...", "options": [{"label": "Yes", "value": "1"}, ...] }
/// When body is provided, bypasses ring buffer parsing (useful for testing/external integrations).
pub async fn hook_permission(
    AxumState(state): AxumState<AppState>,
    Path(raw_pane_id): Path<String>,
    body: Option<Json<serde_json::Value>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pane_id = resolve_pane(&state, &raw_pane_id)
        .map_err(|(s, m)| (s, m.to_string()))?;
    // Hook firing = authoritative proof that an agent is running in this pane.
    ensure_agent_state(&state, &pane_id);

    let explicit_prompt = body.and_then(|Json(b)| {
        let question = b.get("question")?.as_str()?.to_string();
        let options = b.get("options")?.as_array()?.iter().filter_map(|o| {
            Some(crate::multiplexer::types::PromptOption {
                label: o.get("label")?.as_str()?.to_string(),
                value: o.get("value")?.as_str()?.to_string(),
            })
        }).collect::<Vec<_>>();
        if options.is_empty() { return None; }
        Some(crate::multiplexer::types::PendingPrompt { question, options })
    });

    let decision = if let Some(prompt) = explicit_prompt {
        state.svc.prompt_detector.lock().hook_detected_explicit(&pane_id, prompt)
    } else {
        let pty_id = state.svc.mux.lock().get_pane(&pane_id)
            .and_then(|p| p.pty_id.clone());
        let agent_type = state.svc.mux.lock().get_pane(&pane_id)
            .and_then(|p| p.agent_type.clone());

        if let Some(pty_id) = pty_id {
            let at = agent_type.unwrap_or_else(crate::config::default_agent_provider);
            let registry = &state.svc.agent_registry;

            let clean = state.svc.screen_state.lock().get_screen_text(&pty_id).unwrap_or_default();
            log::debug!("[hook_permission] screen_text len={}, agent_type={:?}", clean.len(), at);

            let mut detector = state.svc.prompt_detector.lock();
            let decision = detector.hook_detected_parse(&pane_id, &clean, |text| registry.parse_prompt(&at, text));

            if !matches!(decision, crate::agent::prompt::PromptDecision::None) {
                decision
            } else {
                // Fallback: raw output buffer (preserves UTF-8, contains ANSI).
                // VT100 parser can garble multi-byte chars (C1 byte stripping),
                // so we try the raw buffer with ANSI stripped as a second source.
                let raw = state.svc.output_buf.lock().get_text(&pty_id).unwrap_or_default();
                if raw.is_empty() {
                    decision
                } else {
                    let stripped = crate::pty::ansi::strip_ansi(&raw);
                    let tail = if stripped.len() > 3000 { &stripped[stripped.len() - 3000..] } else { &stripped };
                    log::debug!("[hook_permission] fallback raw buffer len={}, tail_len={}", raw.len(), tail.len());
                    detector.hook_detected_parse(&pane_id, tail, |text| registry.parse_prompt(&at, text))
                }
            }
        } else {
            crate::agent::prompt::PromptDecision::None
        }
    };

    state.svc.apply_prompt_decision(&decision);

    // For hooks, always notify even if decision is None (hook firing means agent is waiting).
    if matches!(decision, crate::agent::prompt::PromptDecision::None) {
        let title = state.svc.mux.lock().get_pane(&pane_id).map(|p| p.title.clone());
        if let Some(title) = title {
            state.svc.notify_event("agent.permission_request", serde_json::json!({
                "pane_id": &pane_id,
                "title": format!("{} needs approval", title),
                "body": "Agent is waiting for your permission",
            }));
        }
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

/// Hook endpoint: agent stopped (task complete).
/// Called by Claude Code hook (Stop matcher).
pub async fn hook_stop(
    AxumState(state): AxumState<AppState>,
    Path(raw_pane_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pane_id = resolve_pane(&state, &raw_pane_id)
        .map_err(|(s, m)| (s, m.to_string()))?;
    let title = state.svc.mux.lock().get_pane(&pane_id).map(|p| p.title.clone());
    if let Some(title) = title {
        state.svc.notify_event("agent.task_complete", serde_json::json!({
            "pane_id": &pane_id,
            "title": format!("{} finished", title),
            "body": "Task complete",
        }));
    }

    // Skip for protected panes (General Agent) — agent_type is permanent there.
    let is_protected = state.svc.mux.lock().get_pane(&pane_id).map(|p| p.protected).unwrap_or(false);
    if !is_protected {
        state.svc.set_pane_agent_type(&pane_id, None);
    }
    state.svc.prompt_detector.lock().clear(&pane_id);
    {
        let mut mux = state.svc.mux.lock();
        if let Some(pane) = mux.get_pane_mut(&pane_id) {
            pane.pending_prompt = None;
        }
    }
    state.svc.emit_state_changed();

    Ok(Json(serde_json::json!({"ok": true})))
}

/// Hook endpoint: SessionStart event from Claude Code. Body is the JSON Claude
/// passes to the hook on stdin (forwarded by the curl command we install).
/// We extract `session_id` and persist it on the pane so future restarts
/// resume the right conversation — covers /new and /clear inside the TUI.
pub async fn hook_session_start(
    AxumState(state): AxumState<AppState>,
    Path(raw_pane_id): Path<String>,
    body: Option<Json<serde_json::Value>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pane_id = resolve_pane(&state, &raw_pane_id)
        .map_err(|(s, m)| (s, m.to_string()))?;
    let body = body.map(|Json(v)| v).unwrap_or(serde_json::Value::Null);
    let session_id = body.get("session_id").and_then(|v| v.as_str()).map(String::from);
    let source = body.get("source").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();

    let Some(sid) = session_id else {
        log::warn!("[hook_session_start] pane={} source={} body had no session_id", pane_id, source);
        return Ok(Json(serde_json::json!({"ok": false, "reason": "no session_id in body"})));
    };

    let prev = state.svc.mux.lock().get_pane(&pane_id).and_then(|p| p.agent_session_id.clone());
    if prev.as_deref() == Some(sid.as_str()) {
        // Same session — no-op (avoids snapshot churn on every Claude turn).
        return Ok(Json(serde_json::json!({"ok": true, "changed": false})));
    }

    log::info!(
        "[hook_session_start] pane={} source={} session {:?} -> {}",
        pane_id, source, prev.as_deref().unwrap_or("-"), sid
    );
    state.svc.set_agent_session_id(&pane_id, Some(sid));
    state.svc.emit_state_changed();
    Ok(Json(serde_json::json!({"ok": true, "changed": true})))
}
