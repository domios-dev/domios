use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::routes::{resolve_pane, AppState};

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionStartClaim {
    prev: Option<String>,
    changed: bool,
    nonce_verified: bool,
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
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
        Some(crate::multiplexer::types::PendingPrompt {
            question,
            options,
            sections: None,
            multi_select: None,
            submit_available: None,
            selected_values: None,
        })
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
    state
        .svc
        .complete_worker_turn_without_report(&pane_id, "provider-stop");

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
    let session_id = session_start_string(&body, &["session_id"])
        .or_else(|| session_start_string(&body, &["payload", "session_id"]))
        .or_else(|| session_start_string(&body, &["payload", "id"]));
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

    let claim = bind_reported_session(
        &state.svc.mux,
        state.svc.agent_registry.as_ref(),
        &pane_id,
        &provider_id,
        &sid,
        report_nonce.as_deref(),
    )?;
    crate::service::general_agent::binding_wait::notify_session_bound();
    acknowledge_authenticated_launch(&state.svc.mux, &pane_id, &claim, report_nonce.as_deref());
    if claim.nonce_verified {
        state.svc.rearm_authenticated_agent_input(&pane_id);
    }

    // while it runs — not only after completion.
    propagate_run_session(&state, &pane_id, &sid);
    let transcript_progress =
        durable_transcript_progress(&state, &pane_id, &provider_id, &sid).await;
    state
        .svc
        .run_watcher
        .observe_durable_transcript(&state.svc, &pane_id, transcript_progress);

    if !claim.changed {
        if claim.nonce_verified {
            state.svc.bind_planner_session(&pane_id);
            state.svc.schedule_planner_replay(&pane_id);
        }
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
    if let Ok(Some(resume)) =
        state
            .svc
            .plan_store
            .observe_handoff_session_start(&pane_id, &sid, epoch_ms())
    {
        state
            .svc
            .send_text_detached(&resume.pane_id, &resume.text, true);
        let _ = state.svc.plan_store.mark_handoff_resume_sent(
            &resume.plan_id,
            &resume.handoff_id,
            epoch_ms(),
        );
        let _ = state.svc.plan_store.ack_handoff_resume(
            &resume.plan_id,
            &resume.handoff_id,
            epoch_ms(),
        );
        crate::api::events::emit_everywhere(
            &state.svc.app_handle,
            "plan-changed",
            serde_json::json!({ "plan_id": resume.plan_id }),
        );
    }
    state.svc.chat_registry.shutdown_pane(&pane_id);
    state.svc.bind_planner_session(&pane_id);
    state.svc.schedule_planner_replay(&pane_id);
    state.svc.emit_state_changed();
    Ok(Json(serde_json::json!({"ok": true, "changed": true})))
}

async fn durable_transcript_progress(
    state: &AppState,
    pane_id: &str,
    provider_id: &str,
    session_id: &str,
) -> bool {
    let cwd = state
        .svc
        .mux
        .read()
        .get_pane(pane_id)
        .and_then(|pane| pane.cwd.clone())
        .unwrap_or_default();
    let registry = state.svc.agent_registry.clone();
    let provider_id = provider_id.to_string();
    let session_id = session_id.to_string();
    tokio::task::spawn_blocking(move || {
        registry
            .with_provider(&provider_id, |provider| {
                let transcript = provider.read_structured_chat(&cwd, &session_id, None);
                crate::agent::run_watcher::has_nonempty_assistant_transcript(&transcript)
            })
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}

fn acknowledge_authenticated_launch(
    mux: &crate::multiplexer::commands::MuxState,
    pane_id: &str,
    claim: &SessionStartClaim,
    report_nonce: Option<&str>,
) -> bool {
    if !claim.nonce_verified {
        return false;
    }
    let Some(binding_nonce) = report_nonce else {
        return false;
    };
    let binding = {
        let mux = mux.read();
        mux.get_pane(pane_id).and_then(|pane| {
            (pane.reported_session_binding_nonce.as_deref() == Some(binding_nonce))
                .then(|| pane.pty_id.clone())
                .flatten()
                .map(|pty_id| crate::service::ProviderSessionBinding {
                    pty_id,
                    binding_nonce: binding_nonce.to_string(),
                })
        })
    };
    binding.is_some_and(|binding| crate::service::record_provider_session(pane_id, &binding))
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn session_start_string(body: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut cur = body;
    for key in path {
        cur = cur.get(*key)?;
    }
    cur.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// Push the just-bound session id (+cwd) onto whatever hidden run owns this pane
/// (mem worker or cron run), so the read-only chat-ui can render it live.
fn propagate_run_session(state: &AppState, pane_id: &str, sid: &str) {
    use tauri::Manager;

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
            crate::api::events::emit_everywhere(
                &state.svc.app_handle,
                "graph-worker-progress",
                &info,
            );
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
            crate::api::events::emit_everywhere(&state.svc.app_handle, "cron-run-finished", ());
        }
    }
}

/// Materialization probe results captured OUTSIDE the mux lock, keyed on the
/// pane fields (`cwd`, current `agent_session_id`) snapshotted just before.
struct MaterializationProbe {
    cwd: String,
    prev_sid: Option<String>,
    new_materialized: bool,
    prev_materialized: bool,
}

fn bind_reported_session(
    mux: &crate::multiplexer::commands::MuxState,
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

    // `session_exists` scans a QuickJS-backed store; under the mux lock that freezes every pane mutation.
    // Snapshot probe inputs, probe unlocked, commit only if the snapshot still holds — a moved binding retries, never overwrites.
    const MAX_ATTEMPTS: usize = 4;
    for _ in 0..MAX_ATTEMPTS {
        let (probe_cwd, probe_prev) = {
            let mux = mux.read();
            let Some(pane) = mux.get_pane(pane_id) else {
                return Err((StatusCode::NOT_FOUND, "pane not found".to_string()));
            };
            (
                pane.cwd.clone().unwrap_or_default(),
                pane.agent_session_id.clone(),
            )
        };

        let new_materialized = registry
            .with_provider(provider_id, |p| p.session_exists(&probe_cwd, sid))
            .unwrap_or(false);
        // A materialized parent protects its rollout from fileless eviction.
        let prev_materialized = if !new_materialized {
            match probe_prev.as_deref() {
                Some(prev) if prev != sid => registry
                    .with_provider(provider_id, |p| p.session_exists(&probe_cwd, prev))
                    .unwrap_or(false),
                _ => false,
            }
        } else {
            false
        };

        let probe = MaterializationProbe {
            cwd: probe_cwd,
            prev_sid: probe_prev,
            new_materialized,
            prev_materialized,
        };

        let mut mux = mux.write();
        match commit_reported_session(&mut mux, pane_id, provider_id, sid, report_nonce, &probe)? {
            Some(claim) => return Ok(claim),
            None => continue, // snapshot went stale under us — reprobe.
        }
    }

    Err((
        StatusCode::CONFLICT,
        "session binding raced concurrent mutation; retry".to_string(),
    ))
}

/// Reacquire-and-commit half of [`bind_reported_session`]. Runs under the write
/// lock with the out-of-lock probe results. Returns `Ok(None)` when the probe
/// snapshot no longer matches live pane state, signalling the caller to reprobe.
fn commit_reported_session(
    mux: &mut crate::multiplexer::state::MultiplexerState,
    pane_id: &str,
    provider_id: &str,
    sid: &str,
    report_nonce: Option<&str>,
    probe: &MaterializationProbe,
) -> Result<Option<SessionStartClaim>, (StatusCode, String)> {
    let Some(pane) = mux.get_pane(pane_id) else {
        return Err((StatusCode::NOT_FOUND, "pane not found".to_string()));
    };
    // The probe keyed on these; if either moved, the probe is stale — reprobe.
    // This is what stops a stale snapshot from overwriting a changed binding.
    if pane.cwd.clone().unwrap_or_default() != probe.cwd || pane.agent_session_id != probe.prev_sid
    {
        return Ok(None);
    }

    // A report carrying a nonce that disagrees with the pane's launch nonce is a stale or foreign reporter (relaunch, /new in another process, reused pane id). A matching nonce is proof we launched this session ourselves — it's authoritative even before the session file lands on disk. A missing report nonce is tolerated only after provider-owned session validation below.
    let launch_nonce = pane.session_binding_nonce.clone();
    let acknowledged_nonce = pane.reported_session_binding_nonce.clone();
    let nonce_verified = if let Some(expected) = pane.session_binding_nonce.clone() {
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

    let exists = nonce_verified || probe.new_materialized;
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

    // A fresh authenticated launch may advance before its rollout lands on disk.
    // An acknowledged nonce cannot evict a materialized parent again.
    if probe.prev_materialized {
        let nonce_already_acknowledged =
            launch_nonce.is_some() && acknowledged_nonce == launch_nonce;
        let fresh_authenticated_launch = nonce_verified && !nonce_already_acknowledged;
        if !fresh_authenticated_launch {
            log::warn!(
                "[hook_session_start] rejecting fileless sid {} for pane {} provider={}: would evict materialized session {:?}",
                sid, pane_id, provider_id, probe.prev_sid,
            );
            return Err((
                StatusCode::CONFLICT,
                format!(
                    "session_id '{}' cannot evict materialized session {:?}",
                    sid, probe.prev_sid
                ),
            ));
        }
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
        crate::service::pane::clear_runtime_model_evidence(pane);
    }
    pane.reported_session_binding_nonce =
        nonce_verified.then(|| report_nonce.unwrap_or_default().to_string());
    Ok(Some(SessionStartClaim {
        prev,
        changed,
        nonce_verified,
    }))
}

#[cfg(test)]
mod tests;
