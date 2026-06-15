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
        let mux = state.svc.mux.lock();
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
            .lock()
            .get_pane(&pane_id)
            .and_then(|p| p.pty_id.clone());
        let agent_type = state
            .svc
            .mux
            .lock()
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
            .lock()
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
    let title = state
        .svc
        .mux
        .lock()
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
        let mut mux = state.svc.mux.lock();
        if let Some(pane) = mux.get_pane_mut(&pane_id) {
            pane.pending_prompt = None;
        }
    }
    state.svc.emit_state_changed();

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
            .lock()
            .get_pane(&pane_id)
            .and_then(|p| p.agent_type.clone());
        provider_id
            .or(existing)
            .unwrap_or_else(|| "claude".to_string())
    };

    let claim = {
        let mut mux = state.svc.mux.lock();
        bind_reported_session(
            &mut mux,
            state.svc.agent_registry.as_ref(),
            &pane_id,
            &provider_id,
            &sid,
        )?
    };

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

fn bind_reported_session(
    mux: &mut crate::multiplexer::state::MultiplexerState,
    registry: &crate::agent::registry::AgentRegistry,
    pane_id: &str,
    provider_id: &str,
    sid: &str,
) -> Result<SessionStartClaim, (StatusCode, String)> {
    if registry.with_provider(provider_id, |_| ()).is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("unknown provider_id '{}'", provider_id),
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
    } else {
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
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::agent::provider::{
        AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery,
    };
    use crate::agent::registry::AgentRegistry;
    use crate::multiplexer::state::MultiplexerState;

    struct HookProvider(&'static str);

    impl AgentProvider for HookProvider {
        fn id(&self) -> &str {
            self.0
        }

        fn display_name(&self) -> &str {
            self.0
        }

        fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
            self.0.to_string()
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

        fn detect_launched_session_id(
            &self,
            _: &str,
            _: &[String],
            _: Option<Duration>,
            _: Option<&str>,
        ) -> Option<String> {
            None
        }

        fn detect_started_session(
            &self,
            _: &str,
            _: &[String],
            _: Option<Duration>,
            _: Option<&str>,
            _: Option<&str>,
        ) -> Option<String> {
            None
        }

        fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
            None
        }

        fn has_hooks(&self) -> bool {
            true
        }
    }

    fn registry(ids: &[&'static str]) -> AgentRegistry {
        let registry = AgentRegistry::new();
        for id in ids {
            registry.register_provider(Box::new(HookProvider(id)));
        }
        registry
    }

    #[test]
    fn reported_session_rebinds_same_pane_for_new_conversation() {
        let (mut mux, pane_id) = MultiplexerState::new();
        let registry = registry(&["codex"]);

        let first = bind_reported_session(&mut mux, &registry, &pane_id, "codex", "sid-a").unwrap();
        assert_eq!(
            first,
            SessionStartClaim {
                prev: None,
                changed: true,
            }
        );

        let second =
            bind_reported_session(&mut mux, &registry, &pane_id, "codex", "sid-b").unwrap();
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
            bind_reported_session(&mut mux, &registry, &pane_id, "codex", "sid-b").unwrap();
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
        let pane_b = mux.create_pane();
        let registry = registry(&["opencode"]);

        bind_reported_session(&mut mux, &registry, &pane_a, "opencode", "ses_shared").unwrap();
        let err = bind_reported_session(&mut mux, &registry, &pane_b, "opencode", "ses_shared")
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
        let registry = registry(&["claude", "codex"]);

        bind_reported_session(&mut mux, &registry, &pane_id, "claude", "sid-a").unwrap();
        let err =
            bind_reported_session(&mut mux, &registry, &pane_id, "codex", "sid-b").unwrap_err();
        assert_eq!(err.0, StatusCode::CONFLICT);
        assert_eq!(
            mux.get_pane(&pane_id).unwrap().agent_session_id.as_deref(),
            Some("sid-a")
        );
    }
}
