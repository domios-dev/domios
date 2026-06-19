use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use crate::api::routes::{resolve_pane, AppState};

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionStartClaim {
    prev: Option<String>,
    changed: bool,
}

/// Ensure agent state is set when a hook fires.
/// Hooks are authoritative — if a hook calls us, an agent IS running.
/// This fixes detection on restored panes, Windows (PowerShell doesn't reset title), etc.
fn ensure_agent_state(state: &AppState, pane_id: &str) {
    log::info!("[ensure_agent_state] called for pane {}", pane_id);
    let agent_type = {
        let mux = state.svc.mux.write();
        let pane = match mux.get_pane(pane_id) {
            Some(p) => p,
            None => return,
        };
        pane.agent_type.clone()
    };

    let agent_changed = agent_type.is_none();
    if agent_changed {
        // TODO(plugin-prep): when plugins can register HTTP routes, move this whole hook handler into plugin/agent_claude/ and let each provider own
        // its own hook endpoints.
        state
            .svc
            .set_and_lock_pane_agent_type(pane_id, "claude".to_string());
        log::info!("[hook] set agent_type=claude for pane {}", pane_id);
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
    let pane_id = resolve_pane(&state, &raw_pane_id).map_err(|(s, m)| (s, m.to_string()))?;
    // Hook firing = authoritative proof that an agent is running in this pane.
    ensure_agent_state(&state, &pane_id);

    let explicit_prompt = body.and_then(|Json(b)| {
        let question = b.get("question")?.as_str()?.to_string();
        let options = b
            .get("options")?
            .as_array()?
            .iter()
            .filter_map(|o| {
                Some(crate::multiplexer::types::PromptOption {
                    label: o.get("label")?.as_str()?.to_string(),
                    value: o.get("value")?.as_str()?.to_string(),
                })
            })
            .collect::<Vec<_>>();
        if options.is_empty() {
            return None;
        }
        Some(crate::multiplexer::types::PendingPrompt { question, options })
    });

    let decision = if let Some(prompt) = explicit_prompt {
        state
            .svc
            .prompt_detector
            .lock()
            .hook_detected_explicit(&pane_id, prompt)
    } else {
        let pty_id = state
            .svc
            .mux
            .write()
            .get_pane(&pane_id)
            .and_then(|p| p.pty_id.clone());
        let agent_type = state
            .svc
            .mux
            .write()
            .get_pane(&pane_id)
            .and_then(|p| p.agent_type.clone());

        if let Some(pty_id) = pty_id {
            let at = agent_type.unwrap_or_else(crate::config::default_agent_provider);
            let registry = &state.svc.agent_registry;

            let clean = state
                .svc
                .screen_state
                .lock()
                .get_screen_text(&pty_id)
                .unwrap_or_default();
            log::debug!(
                "[hook_permission] screen_text len={}, agent_type={:?}",
                clean.len(),
                at
            );

            let parsed =
                crate::agent::prompt::parse_prompt_safely(|| registry.parse_prompt(&at, &clean));
            let tail = if parsed.is_none() && clean.len() > 100 {
                Some(crate::agent::prompt::utf8_tail(&clean, 300).to_string())
            } else {
                None
            };
            let decision = state.svc.prompt_detector.lock().hook_detected_parsed(
                &pane_id,
                clean.len(),
                tail.as_deref(),
                parsed,
            );

            if !matches!(decision, crate::agent::prompt::PromptDecision::None) {
                decision
            } else {
                // Fallback: raw output buffer (preserves UTF-8, contains ANSI). VT100 parser can garble multi-byte chars (C1 byte stripping), so we try the
                // raw buffer with ANSI stripped as a second source.
                let raw = state
                    .svc
                    .output_buf
                    .lock()
                    .get_text(&pty_id)
                    .unwrap_or_default();
                if raw.is_empty() {
                    decision
                } else {
                    let stripped = crate::pty::ansi::strip_ansi(&raw);
                    let tail = crate::agent::prompt::utf8_tail(&stripped, 3000);
                    log::debug!(
                        "[hook_permission] fallback raw buffer len={}, tail_len={}",
                        raw.len(),
                        tail.len()
                    );
                    let parsed = crate::agent::prompt::parse_prompt_safely(|| {
                        registry.parse_prompt(&at, tail)
                    });
                    state.svc.prompt_detector.lock().hook_detected_parsed(
                        &pane_id,
                        tail.len(),
                        Some(crate::agent::prompt::utf8_tail(tail, 300)),
                        parsed,
                    )
                }
            }
        } else {
            crate::agent::prompt::PromptDecision::None
        }
    };

    state.svc.apply_prompt_decision(&decision);

    // For hooks, always notify even if decision is None (hook firing means agent is waiting).
    if matches!(decision, crate::agent::prompt::PromptDecision::None) {
        let title = state
            .svc
            .mux
            .write()
            .get_pane(&pane_id)
            .map(|p| p.title.clone());
        if let Some(title) = title {
            state.svc.notify_event(
                "agent.permission_request",
                serde_json::json!({
                    "pane_id": &pane_id,
                    "title": format!("{} needs approval", title),
                    "body": "Agent is waiting for your permission",
                }),
            );
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
    let pane_id = resolve_pane(&state, &raw_pane_id).map_err(|(s, m)| (s, m.to_string()))?;
    log::debug!("[hook_stop] fired for pane {}", pane_id);
    let title = state
        .svc
        .mux
        .write()
        .get_pane(&pane_id)
        .map(|p| p.title.clone());
    if let Some(title) = title {
        state.svc.notify_event(
            "agent.task_complete",
            serde_json::json!({
                "pane_id": &pane_id,
                "title": format!("{} finished", title),
                "body": "Task complete",
            }),
        );
    }

    // Stop = turn boundary, not exit — agent_type stays; PTY-close clears it.
    state.svc.prompt_detector.lock().clear(&pane_id);
    {
        let mut mux = state.svc.mux.write();
        if let Some(pane) = mux.get_pane_mut(&pane_id) {
            pane.pending_prompt = None;
        }
    }
    state.svc.emit_state_changed();

    // Hidden worker/cron runs (now ordinary interactive claude) finish on Stop if they didn't explicitly report. No-op for visible panes / already-reported runs.
    state
        .svc
        .run_watcher
        .complete_via_stop_hook(&state.svc, &pane_id);

    Ok(Json(serde_json::json!({"ok": true})))
}

/// Hook endpoint: provider-reported SessionStart event. Body is the JSON the
/// provider passes to the hook/plugin callback.
/// We extract `session_id` and persist it on the pane so future restarts
/// resume the right conversation — covers /new and /clear inside the TUI.
pub async fn hook_session_start(
    AxumState(state): AxumState<AppState>,
    Path(raw_pane_id): Path<String>,
    headers: HeaderMap,
    body: Option<Json<serde_json::Value>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pane_id = resolve_pane(&state, &raw_pane_id).map_err(|(s, m)| (s, m.to_string()))?;
    let body = body.map(|Json(v)| v).unwrap_or(serde_json::Value::Null);
    let session_id = body
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from);
    let source = body
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let provider_id = body
        .get("provider_id")
        .or_else(|| body.get("provider"))
        .and_then(|v| v.as_str())
        .or_else(|| header_str(&headers, "x-codeterm-agent-provider"))
        .map(|s| s.to_string());
    let report_nonce = body
        .get("binding_nonce")
        .and_then(|v| v.as_str())
        .or_else(|| header_str(&headers, "x-codeterm-binding-nonce"))
        .map(|s| s.to_string());

    let Some(sid) = session_id else {
        log::warn!(
            "[hook_session_start] pane={} source={} body had no session_id",
            pane_id,
            source
        );
        return Ok(Json(
            serde_json::json!({"ok": false, "reason": "no session_id in body"}),
        ));
    };

    let provider_id = {
        let existing = state
            .svc
            .mux
            .write()
            .get_pane(&pane_id)
            .and_then(|p| p.agent_type.clone());
        provider_id
            .or(existing)
            .unwrap_or_else(|| "claude".to_string())
    };

    let claim = {
        let mut mux = state.svc.mux.write();
        bind_reported_session(
            &mut mux,
            state.svc.agent_registry.as_ref(),
            &pane_id,
            &provider_id,
            &sid,
            report_nonce.as_deref(),
        )?
    };

    // Capture the freshly-bound session onto the hidden run's record (worker / cron) immediately, so a LIVE run is renderable in the read-only chat-ui
    // while it runs — not only after completion.
    propagate_run_session(&state, &pane_id, &sid);

    if !claim.changed {
        return Ok(Json(serde_json::json!({"ok": true, "changed": false})));
    }

    log::info!(
        "[hook_session_start] pane={} provider={} source={} session {:?} -> {}",
        pane_id,
        provider_id,
        source,
        claim.prev.as_deref().unwrap_or("-"),
        sid
    );
    state.svc.chat_registry.shutdown_pane(&pane_id);
    state.svc.emit_state_changed();
    Ok(Json(serde_json::json!({"ok": true, "changed": true})))
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// Push the just-bound session id (+cwd) onto whatever hidden run owns this pane
/// (mem worker or cron run), so the read-only chat-ui can render it live.
fn propagate_run_session(state: &AppState, pane_id: &str, sid: &str) {
    use tauri::{Emitter, Manager};

    let (worker_id, cwd) = {
        let mux = state.svc.mux.write();
        let pane = mux.get_pane(pane_id);
        (
            pane.and_then(|p| p.worker_id.clone()),
            pane.and_then(|p| p.cwd.clone()),
        )
    };

    if let Some(wid) = worker_id {
        state
            .svc
            .workers
            .set_run_session(&wid, sid.to_string(), cwd.clone());
        if let Some(info) = state.svc.workers.get(&wid) {
            let _ = state.svc.app_handle.emit("graph-worker-progress", &info);
        }
    }

    if let Some(cron_state) = state.svc.app_handle.try_state::<crate::CronState>() {
        let owner = cron_state
            .lock()
            .get_pane_owner(pane_id)
            .map(|(j, r)| (j.to_string(), r.to_string()));
        if let Some((job_id, run_id)) = owner {
            cron_state
                .lock()
                .set_run_session(&job_id, &run_id, sid, cwd.as_deref());
            let _ = state.svc.app_handle.emit("cron-state-changed", ());
        }
    }
}

fn bind_reported_session(
    mux: &mut crate::multiplexer::state::MultiplexerState,
    registry: &crate::agent::registry::AgentRegistry,
    pane_id: &str,
    provider_id: &str,
    sid: &str,
    report_nonce: Option<&str>,
) -> Result<SessionStartClaim, (StatusCode, String)> {
    if registry.with_provider(provider_id, |_| ()).is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("unknown provider_id '{}'", provider_id),
        ));
    }

    // A report carrying a nonce that disagrees with the pane's launch nonce is a stale or foreign reporter (relaunch, /new in another process, reused pane id). A matching nonce is proof we launched this session ourselves — it's authoritative even before the session file lands on disk. A missing report nonce is tolerated only after provider-owned session validation below.
    let nonce_verified = if let Some(expected) = mux
        .get_pane(pane_id)
        .and_then(|p| p.session_binding_nonce.clone())
    {
        match report_nonce {
            Some(got) if got != expected => {
                log::warn!(
                    "[hook_session_start] rejecting sid {} for pane {} provider={}: binding nonce mismatch",
                    sid, pane_id, provider_id,
                );
                return Err((StatusCode::CONFLICT, "binding nonce mismatch".to_string()));
            }
            Some(_) => true,
            None => false,
        }
    } else {
        false
    };

    let Some(pane) = mux.get_pane(pane_id) else {
        return Err((StatusCode::NOT_FOUND, "pane not found".to_string()));
    };
    if let Some(existing_provider) = pane.agent_type.as_deref() {
        if existing_provider != provider_id {
            return Err((
                StatusCode::CONFLICT,
                format!(
                    "pane provider '{}' does not match report provider '{}'",
                    existing_provider, provider_id
                ),
            ));
        }
    }
    let cwd = pane.cwd.clone().unwrap_or_default();

    // A verified launch nonce already proves ownership; the session file just hasn't
    // been persisted yet (fresh agent spawn at session_start). Only gate nonce-less /
    // unverified reports on provider-owned session existence — that's the CB181
    // cross-contamination guard for genuinely *reported* (not launched) sessions.
    let exists = nonce_verified
        || registry
            .with_provider(provider_id, |p| p.session_exists(&cwd, sid))
            .unwrap_or(false);
    if !exists {
        log::warn!(
            "[hook_session_start] rejecting sid {} for pane {} provider={}: session does not belong to provider",
            sid,
            pane_id,
            provider_id,
        );
        return Err((
            StatusCode::CONFLICT,
            format!(
                "session_id '{}' is not a known session for provider '{}'",
                sid, provider_id
            ),
        ));
    }

    let conflict = mux
        .pane_ids_with_session(sid)
        .into_iter()
        .find(|other| other != pane_id);
    if let Some(other) = conflict {
        log::warn!(
            "[hook_session_start] rejecting sid {} for pane {} provider={} already claimed by {}",
            sid,
            pane_id,
            provider_id,
            other,
        );
        return Err((
            StatusCode::CONFLICT,
            format!("session_id already claimed by pane {}", other),
        ));
    }

    let Some(pane) = mux.get_pane_mut(pane_id) else {
        return Err((StatusCode::NOT_FOUND, "pane not found".to_string()));
    };
    if pane.agent_type.is_none() {
        pane.agent_type = Some(provider_id.to_string());
        pane.agent_type_locked = true;
        pane.agent_set_at = Some(std::time::Instant::now());
    }

    let prev = pane.agent_session_id.clone();
    let changed = prev.as_deref() != Some(sid);
    if changed {
        pane.agent_session_id = Some(sid.to_string());
    }
    Ok(SessionStartClaim { prev, changed })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::agent::provider::{
        AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery,
    };
    use crate::agent::registry::AgentRegistry;
    use crate::multiplexer::state::MultiplexerState;

    struct HookProvider {
        id: &'static str,
        sessions: HashSet<&'static str>,
    }

    impl AgentProvider for HookProvider {
        fn id(&self) -> &str {
            self.id
        }

        fn display_name(&self) -> &str {
            self.id
        }

        fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
            self.id.to_string()
        }

        fn readiness_strategy(&self) -> ReadinessStrategy {
            ReadinessStrategy::FixedDelay {
                duration: Duration::ZERO,
            }
        }

        fn task_delivery(&self) -> TaskDelivery {
            TaskDelivery::PtyInput
        }

        fn detect_from_title(&self, _: &str) -> bool {
            false
        }

        fn detect_from_output(&self, _: &str) -> bool {
            false
        }

        fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
            None
        }

        fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
            None
        }

        fn session_exists(&self, _: &str, session_id: &str) -> bool {
            self.sessions.contains(session_id)
        }

        fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
            None
        }

        fn has_hooks(&self) -> bool {
            true
        }
    }

    fn registry(ids: &[&'static str]) -> AgentRegistry {
        registry_with_sessions(
            ids.iter()
                .map(|id| (*id, vec!["sid-a", "sid-b", "ses_shared"]))
                .collect(),
        )
    }

    fn registry_with_sessions(items: Vec<(&'static str, Vec<&'static str>)>) -> AgentRegistry {
        let registry = AgentRegistry::new();
        for (id, sessions) in items {
            registry.register_provider(Box::new(HookProvider {
                id,
                sessions: sessions.into_iter().collect(),
            }));
        }
        registry
    }

    #[test]
    fn reported_session_rebinds_same_pane_for_new_conversation() {
        let (mut mux, pane_id) = MultiplexerState::new();
        mux.get_pane_mut(&pane_id).unwrap().cwd = Some("/tmp/project".into());
        let registry = registry(&["codex"]);

        let first =
            bind_reported_session(&mut mux, &registry, &pane_id, "codex", "sid-a", None).unwrap();
        assert_eq!(
            first,
            SessionStartClaim {
                prev: None,
                changed: true,
            }
        );

        let second =
            bind_reported_session(&mut mux, &registry, &pane_id, "codex", "sid-b", None).unwrap();
        assert_eq!(
            second,
            SessionStartClaim {
                prev: Some("sid-a".into()),
                changed: true,
            }
        );
        let pane = mux.get_pane(&pane_id).unwrap();
        assert_eq!(pane.agent_type.as_deref(), Some("codex"));
        assert_eq!(pane.agent_session_id.as_deref(), Some("sid-b"));
        assert!(pane.agent_type_locked);

        let unchanged =
            bind_reported_session(&mut mux, &registry, &pane_id, "codex", "sid-b", None).unwrap();
        assert_eq!(
            unchanged,
            SessionStartClaim {
                prev: Some("sid-b".into()),
                changed: false,
            }
        );
    }

    #[test]
    fn reported_session_rejects_cross_pane_duplicate_claim() {
        let (mut mux, pane_a) = MultiplexerState::new();
        mux.get_pane_mut(&pane_a).unwrap().cwd = Some("/tmp/project".into());
        let pane_b = mux.create_pane();
        mux.get_pane_mut(&pane_b).unwrap().cwd = Some("/tmp/project".into());
        let registry = registry(&["opencode"]);

        bind_reported_session(&mut mux, &registry, &pane_a, "opencode", "ses_shared", None)
            .unwrap();
        let err =
            bind_reported_session(&mut mux, &registry, &pane_b, "opencode", "ses_shared", None)
                .unwrap_err();
        assert_eq!(err.0, StatusCode::CONFLICT);
        assert_eq!(
            mux.get_pane(&pane_b).unwrap().agent_session_id.as_deref(),
            None
        );
    }

    #[test]
    fn reported_session_rejects_wrong_provider_for_locked_pane() {
        let (mut mux, pane_id) = MultiplexerState::new();
        mux.get_pane_mut(&pane_id).unwrap().cwd = Some("/tmp/project".into());
        let registry = registry(&["claude", "codex"]);

        bind_reported_session(&mut mux, &registry, &pane_id, "claude", "sid-a", None).unwrap();
        let err = bind_reported_session(&mut mux, &registry, &pane_id, "codex", "sid-b", None)
            .unwrap_err();
        assert_eq!(err.0, StatusCode::CONFLICT);
        assert_eq!(
            mux.get_pane(&pane_id).unwrap().agent_session_id.as_deref(),
            Some("sid-a")
        );
    }

    #[test]
    fn reported_session_enforces_binding_nonce() {
        let (mut mux, pane_id) = MultiplexerState::new();
        mux.get_pane_mut(&pane_id).unwrap().cwd = Some("/tmp/project".into());
        let registry = registry(&["claude"]);
        mux.get_pane_mut(&pane_id).unwrap().session_binding_nonce = Some("nonce-1".into());

        // Wrong nonce → rejected, no binding.
        let err = bind_reported_session(
            &mut mux,
            &registry,
            &pane_id,
            "claude",
            "sid-a",
            Some("nope"),
        )
        .unwrap_err();
        assert_eq!(err.0, StatusCode::CONFLICT);
        assert_eq!(
            mux.get_pane(&pane_id).unwrap().agent_session_id.as_deref(),
            None
        );

        // Absent nonce → tolerated (un-upgraded hook).
        bind_reported_session(&mut mux, &registry, &pane_id, "claude", "sid-a", None).unwrap();
        assert_eq!(
            mux.get_pane(&pane_id).unwrap().agent_session_id.as_deref(),
            Some("sid-a")
        );

        // Matching nonce → bound.
        let claim = bind_reported_session(
            &mut mux,
            &registry,
            &pane_id,
            "claude",
            "sid-b",
            Some("nonce-1"),
        )
        .unwrap();
        assert!(claim.changed);
        assert_eq!(
            mux.get_pane(&pane_id).unwrap().agent_session_id.as_deref(),
            Some("sid-b")
        );
    }

    #[test]
    fn reported_session_rejects_sid_not_owned_by_provider() {
        let (mut mux, pane_id) = MultiplexerState::new();
        mux.get_pane_mut(&pane_id).unwrap().cwd = Some("/tmp/project".into());
        let registry = registry_with_sessions(vec![
            ("claude", vec!["claude-real"]),
            ("cursor-agent", vec!["cursor-foreign"]),
        ]);

        let err = bind_reported_session(
            &mut mux,
            &registry,
            &pane_id,
            "claude",
            "cursor-foreign",
            Some("nonce-1"),
        )
        .unwrap_err();

        assert_eq!(err.0, StatusCode::CONFLICT);
        let pane = mux.get_pane(&pane_id).unwrap();
        assert_eq!(pane.agent_type.as_deref(), None);
        assert_eq!(pane.agent_session_id.as_deref(), None);
    }

    #[test]
    fn reported_session_binds_sid_owned_by_provider() {
        let (mut mux, pane_id) = MultiplexerState::new();
        mux.get_pane_mut(&pane_id).unwrap().cwd = Some("/tmp/project".into());
        let registry = registry_with_sessions(vec![("claude", vec!["claude-real"])]);

        bind_reported_session(
            &mut mux,
            &registry,
            &pane_id,
            "claude",
            "claude-real",
            Some("nonce-1"),
        )
        .unwrap();

        let pane = mux.get_pane(&pane_id).unwrap();
        assert_eq!(pane.agent_type.as_deref(), Some("claude"));
        assert_eq!(pane.agent_session_id.as_deref(), Some("claude-real"));
    }

    #[test]
    fn fresh_launch_with_matching_nonce_accepts_unpersisted_session() {
        // P0.2 regression: a session WE just launched carries a valid per-launch
        // nonce but is not yet on disk at session_start time. A matching nonce is
        // proof-of-ownership — it must be accepted even though session_exists==false.
        let (mut mux, pane_id) = MultiplexerState::new();
        let pane = mux.get_pane_mut(&pane_id).unwrap();
        pane.cwd = Some("/tmp/project".into());
        pane.session_binding_nonce = Some("launch-nonce".into());
        // Provider knows of NO persisted session for this sid.
        let registry = registry_with_sessions(vec![("claude", vec![])]);

        let claim = bind_reported_session(
            &mut mux,
            &registry,
            &pane_id,
            "claude",
            "fresh-unpersisted-sid",
            Some("launch-nonce"),
        )
        .expect("fresh launch with matching nonce must be accepted");
        assert!(claim.changed);

        let pane = mux.get_pane(&pane_id).unwrap();
        assert_eq!(pane.agent_type.as_deref(), Some("claude"));
        assert_eq!(
            pane.agent_session_id.as_deref(),
            Some("fresh-unpersisted-sid")
        );
    }

    #[test]
    fn pane_with_nonce_but_nonceless_report_rejects_unpersisted_session() {
        // CB181 preserved: without a matching nonce, an unknown/unpersisted sid is
        // still gated by session_exists — a nonce-less report is not proof-of-ownership.
        let (mut mux, pane_id) = MultiplexerState::new();
        let pane = mux.get_pane_mut(&pane_id).unwrap();
        pane.cwd = Some("/tmp/project".into());
        pane.session_binding_nonce = Some("launch-nonce".into());
        let registry = registry_with_sessions(vec![("claude", vec![])]);

        let err =
            bind_reported_session(&mut mux, &registry, &pane_id, "claude", "unknown-sid", None)
                .unwrap_err();
        assert_eq!(err.0, StatusCode::CONFLICT);
        let pane = mux.get_pane(&pane_id).unwrap();
        assert_eq!(pane.agent_session_id.as_deref(), None);
    }

    #[test]
    fn fresh_launch_with_matching_nonce_still_rejects_already_claimed_sid() {
        // A valid nonce skips session_exists, but the already-claimed guard must
        // still fire — two panes cannot own the same live session.
        let (mut mux, pane_a) = MultiplexerState::new();
        mux.get_pane_mut(&pane_a).unwrap().cwd = Some("/tmp/project".into());
        let pane_b = mux.create_pane();
        {
            let pane = mux.get_pane_mut(&pane_b).unwrap();
            pane.cwd = Some("/tmp/project".into());
            pane.session_binding_nonce = Some("launch-nonce".into());
        }
        let registry = registry_with_sessions(vec![("claude", vec!["ses_shared"])]);

        bind_reported_session(&mut mux, &registry, &pane_a, "claude", "ses_shared", None).unwrap();
        let err = bind_reported_session(
            &mut mux,
            &registry,
            &pane_b,
            "claude",
            "ses_shared",
            Some("launch-nonce"),
        )
        .unwrap_err();
        assert_eq!(err.0, StatusCode::CONFLICT);
        assert_eq!(
            mux.get_pane(&pane_b).unwrap().agent_session_id.as_deref(),
            None
        );
    }

    #[test]
    fn reported_session_without_nonce_rejects_foreign_sid() {
        let (mut mux, pane_id) = MultiplexerState::new();
        let pane = mux.get_pane_mut(&pane_id).unwrap();
        pane.cwd = Some("/tmp/project".into());
        pane.session_binding_nonce = Some("nonce-1".into());
        let registry = registry_with_sessions(vec![
            ("claude", vec!["claude-real"]),
            ("cursor-agent", vec!["cursor-foreign"]),
        ]);

        let err = bind_reported_session(
            &mut mux,
            &registry,
            &pane_id,
            "claude",
            "cursor-foreign",
            None,
        )
        .unwrap_err();

        assert_eq!(err.0, StatusCode::CONFLICT);
        let pane = mux.get_pane(&pane_id).unwrap();
        assert_eq!(pane.agent_type.as_deref(), None);
        assert_eq!(pane.agent_session_id.as_deref(), None);
    }
}
