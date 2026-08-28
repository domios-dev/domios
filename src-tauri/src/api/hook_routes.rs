use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::agent::provider::{HookEventBinding, HookIngestionRoute};
use crate::agent::registry::AgentRegistry;
use crate::api::{optional_json::OptionalJson, routes::{resolve_tab, AppState}};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionStartClaim {
    prev: Option<String>,
    changed: bool,
    nonce_verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeferredBindingPremise {
    previous_session_id: String,
}

pub(crate) type SessionBindError = (StatusCode, String, Option<DeferredBindingPremise>);

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The hook route table core registers: each provider's declared roster,
/// verbatim. Nothing undeclared appears here, so nothing undeclared routes.
fn registered_hook_routes(registry: &AgentRegistry) -> BTreeMap<String, Vec<HookEventBinding>> {
    registry
        .provider_ids()
        .into_iter()
        .filter_map(|id| {
            let roster = registry.with_provider(&id, |p| p.hook_event_roster())?;
            (!roster.is_empty()).then_some((id, roster))
        })
        .collect()
}

/// Sole provider whose roster declares `route` — the owning descriptor hook
/// ingestion may attribute to. Unclaimed or contested routes yield None.
fn sole_hook_route_declarer(registry: &AgentRegistry, route: HookIngestionRoute) -> Option<String> {
    let mut declarers = registered_hook_routes(registry)
        .into_iter()
        .filter(|(_, roster)| roster.iter().any(|binding| binding.route == route))
        .map(|(id, _)| id);
    let sole = declarers.next()?;
    declarers.next().is_none().then_some(sole)
}

/// Ensure agent state is set when a hook fires.
/// Hooks are authoritative — if a hook calls us, an agent IS running.
/// This fixes detection on restored tabs, Windows (PowerShell doesn't reset title), etc.
fn ensure_agent_state(state: &AppState, tab_id: &str) {
    log::info!("[ensure_agent_state] called for Tab {}", tab_id);
    let agent_type = {
        let mux = state.svc.mux.write();
        let tab = match mux.get_tab(tab_id) {
            Some(p) => p,
            None => return,
        };
        tab.agent_type.clone()
    };
    if agent_type.is_some() {
        return;
    }

    let Some(owner) =
        sole_hook_route_declarer(&state.svc.agent_registry, HookIngestionRoute::Permission)
    else {
        log::warn!(
            "[hook] Tab {} fired the permission route without a sole declaring provider; agent_type left unset",
            tab_id
        );
        return;
    };
    state.svc.set_and_lock_tab_agent_type(tab_id, owner.clone());
    log::info!("[hook] set agent_type={} for Tab {}", owner, tab_id);
    state.svc.emit_state_changed();
    state.svc.emit_session_binding_state(tab_id);
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
    Path(raw_tab_id): Path<String>,
    headers: HeaderMap,
    OptionalJson(body): OptionalJson<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let tab_id = resolve_tab(&state, &raw_tab_id).map_err(|(s, m)| (s, m.to_string()))?;
    // Hook firing = authoritative proof that an agent is running in this tab.
    ensure_agent_state(&state, &tab_id);

    // The per-tool payload the vendor already sent beats anything scraped back
    // off the screen, so it is ingested before the parsing fallbacks.
    if let Some(raw) = body.as_ref() {
        if let Some(signal) = crate::agent::permission_request::parse_permission_request(raw) {
            reject_foreign_binding_nonce(&state, &tab_id, &headers, raw)?;
            let decision = state
                .svc
                .prompt_detector
                .lock()
                .hook_detected_permission_request(&tab_id, signal);
            state.svc.apply_prompt_decision(&decision);
            return Ok(Json(serde_json::json!({"ok": true, "ingested": true})));
        }
    }

    let explicit_prompt = body.and_then(|b| {
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
            .hook_detected_explicit(&tab_id, prompt)
    } else {
        let pty_id = state
            .svc
            .mux
            .write()
            .get_tab(&tab_id)
            .and_then(|p| p.pty_id.clone());
        let agent_type = state
            .svc
            .mux
            .write()
            .get_tab(&tab_id)
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
                &tab_id,
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
                        &tab_id,
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

    Ok(Json(serde_json::json!({"ok": true})))
}

/// Verdict for a provider-reported Stop POST. A carried nonce must equal the
/// tab's own launch nonce (completion-via-report); a nonce-less POST stays on
/// the legacy inference-tolerant path; anything else is a foreign reporter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopReportVerdict {
    Verified,
    Legacy,
    Foreign,
}

fn stop_report_verdict(
    launch_nonce: Option<&str>,
    report_nonce: Option<&str>,
) -> StopReportVerdict {
    match (launch_nonce, report_nonce) {
        (Some(expected), Some(got)) if got == expected => StopReportVerdict::Verified,
        (_, Some(_)) => StopReportVerdict::Foreign,
        (_, None) => StopReportVerdict::Legacy,
    }
}

/// Hook endpoint: agent stopped (task complete). Called by the Stop hook of
/// whichever provider declares the stop route in its roster.
pub async fn hook_stop(
    AxumState(state): AxumState<AppState>,
    Path(raw_tab_id): Path<String>,
    headers: HeaderMap,
    OptionalJson(body): OptionalJson<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let tab_id = resolve_tab(&state, &raw_tab_id).map_err(|(s, m)| (s, m.to_string()))?;
    log::debug!("[hook_stop] fired for Tab {}", tab_id);
    let body = body.map(|Json(v)| v).unwrap_or(serde_json::Value::Null);
    let report_nonce = body
        .get("binding_nonce")
        .and_then(|v| v.as_str())
        .or_else(|| header_str(&headers, "x-codeterm-binding-nonce"))
        .map(|s| s.to_string());
    let launch_nonce = state
        .svc
        .mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| tab.session_binding_nonce.clone());
    let verdict = stop_report_verdict(launch_nonce.as_deref(), report_nonce.as_deref());
    if verdict == StopReportVerdict::Foreign {
        log::warn!(
            "[hook_stop] rejecting stop for Tab {}: binding nonce mismatch",
            tab_id
        );
        return Err((StatusCode::CONFLICT, "binding nonce mismatch".to_string()));
    }
    let title = state
        .svc
        .mux
        .write()
        .get_tab(&tab_id)
        .map(|p| p.title.clone());
    if let Some(title) = title {
        state.svc.notify_event(
            "agent.task_complete",
            serde_json::json!({
                "tab_id": &tab_id,
                "title": format!("{} finished", title),
                "body": "Task complete",
            }),
        );
    }

    // Stop = turn boundary, not exit — agent_type stays; PTY-close clears it.
    state.svc.prompt_detector.lock().clear(&tab_id);
    {
        let mut mux = state.svc.mux.write();
        if let Some(tab) = mux.get_tab_mut(&tab_id) {
            tab.pending_prompt = None;
        }
    }
    state.svc.emit_state_changed();

    // Hidden worker/cron runs finish on Stop if they didn't explicitly report.
    // A nonce-verified report is the completion-via-report path; a nonce-less
    // legacy post keeps today's tab-keyed behavior. No-op for visible tabs.
    match (verdict, report_nonce.as_deref()) {
        (StopReportVerdict::Verified, Some(nonce)) => state
            .svc
            .run_watcher
            .complete_via_stop_report(&state.svc, &tab_id, nonce),
        _ => state
            .svc
            .run_watcher
            .complete_via_stop_hook(&state.svc, &tab_id),
    }
    state.svc.complete_worker_turn_without_report(
        &tab_id,
        crate::service::WorkerTurnSignal::ProviderStop,
    );

    Ok(Json(serde_json::json!({"ok": true})))
}

/// Hook endpoint: provider-reported SessionStart event. Body is the JSON the
/// provider passes to the hook/plugin callback.
/// We extract `session_id` and persist it on the tab so future restarts
/// resume the right conversation — covers /new and /clear inside the TUI.
pub async fn hook_session_start(
    AxumState(state): AxumState<AppState>,
    Path(raw_tab_id): Path<String>,
    headers: HeaderMap,
    OptionalJson(body): OptionalJson<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let tab_id = resolve_tab(&state, &raw_tab_id).map_err(|(s, m)| (s, m.to_string()))?;
    let body = body.unwrap_or(serde_json::Value::Null);
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
            "[hook_session_start] tab={} source={} body had no session_id",
            tab_id,
            source
        );
        state.svc.emit_session_binding_state(&tab_id);
        return Ok(Json(
            serde_json::json!({"ok": false, "reason": "no session_id in body"}),
        ));
    };

    let provider_id = {
        let existing = state
            .svc
            .mux
            .write()
            .get_tab(&tab_id)
            .and_then(|p| p.agent_type.clone());
        // Identity ladder: report > tab > the route's sole declaring descriptor
        // > configured default. Never a provider-id literal.
        provider_id
            .or(existing)
            .or_else(|| {
                sole_hook_route_declarer(
                    &state.svc.agent_registry,
                    HookIngestionRoute::SessionStart,
                )
            })
            .unwrap_or_else(crate::config::default_agent_provider)
    };

    let claim = match bind_reported_session(
        &state.svc.mux,
        state.svc.agent_registry.as_ref(),
        &tab_id,
        &provider_id,
        &sid,
        report_nonce.as_deref(),
    ) {
        Ok(claim) => claim,
        Err((status, message, Some(premise))) => {
            let transcript_path = reported_transcript_path(&body)
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    state
                        .svc
                        .agent_registry
                        .with_provider(&provider_id, |provider| {
                            let cwd = state
                                .svc
                                .mux
                                .read()
                                .get_tab(&tab_id)
                                .and_then(|tab| tab.cwd.clone())
                                .unwrap_or_default();
                            provider.session_file_path(&cwd, &sid)
                        })
                        .flatten()
                });
            let Some(transcript_path) = transcript_path else {
                state
                    .svc
                    .deferred_session_bindings
                    .refuse(&tab_id, &sid, Some(&premise.previous_session_id), &message);
                state.svc.emit_session_binding_state(&tab_id);
                return Err((status, message));
            };
            if let Err(error) = defer_session_binding(
                state.clone(),
                &tab_id,
                &provider_id,
                &sid,
                &premise.previous_session_id,
                transcript_path,
                report_nonce.clone(),
                source.clone(),
                body.clone(),
            ) {
                let reason = format!("{message}; {error}");
                state
                    .svc
                    .deferred_session_bindings
                    .refuse(&tab_id, &sid, Some(&premise.previous_session_id), &reason);
                state.svc.emit_session_binding_state(&tab_id);
                return Err((StatusCode::CONFLICT, reason));
            }
            state.svc.emit_session_binding_state(&tab_id);
            return Ok(Json(serde_json::json!({
                "ok": true,
                "changed": false,
                "deferred": true,
                "session_id": sid,
                "current_session_id": premise.previous_session_id,
            })));
        }
        Err((status, message, None)) => {
            let current_session_id = state
                .svc
                .mux
                .read()
                .get_tab(&tab_id)
                .and_then(|tab| tab.agent_session_id.clone());
            state
                .svc
                .deferred_session_bindings
                .refuse(&tab_id, &sid, current_session_id.as_deref(), &message);
            state.svc.emit_session_binding_state(&tab_id);
            return Err((status, message));
        }
    };
    finish_session_binding(
        &state,
        &tab_id,
        &provider_id,
        &sid,
        report_nonce.as_deref(),
        &source,
        &body,
        claim,
    )
    .await
}

pub(crate) fn defer_session_binding(
    state: AppState,
    tab_id: &str,
    provider_id: &str,
    sid: &str,
    previous_session_id: &str,
    transcript_path: std::path::PathBuf,
    report_nonce: Option<String>,
    source: String,
    body: serde_json::Value,
) -> Result<(), String> {
    let callback_state = state.clone();
    let callback_nonce = report_nonce.clone();
    let callback_source = source.clone();
    let callback_body = body.clone();
    let on_materialized = std::sync::Arc::new(move |tab_id: String, generation: u64| {
        let state = callback_state.clone();
        let report_nonce = callback_nonce.clone();
        let source = callback_source.clone();
        let body = callback_body.clone();
        tauri::async_runtime::spawn(async move {
            complete_deferred_session_binding(
                state,
                tab_id,
                generation,
                report_nonce,
                source,
                body,
            )
            .await;
        });
    });
    let binding = state.svc.deferred_session_bindings.defer(
        tab_id,
        provider_id,
        sid,
        previous_session_id,
        transcript_path,
        on_materialized.clone(),
    )?;
    if binding.transcript_path.is_file() {
        on_materialized(binding.tab_id, binding.generation);
    }
    Ok(())
}

async fn complete_deferred_session_binding(
    state: AppState,
    tab_id: String,
    generation: u64,
    report_nonce: Option<String>,
    source: String,
    body: serde_json::Value,
) {
    let Some(binding) = state.svc.deferred_session_bindings.get(&tab_id, generation) else {
        return;
    };
    let current_session = state
        .svc
        .mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| tab.agent_session_id.clone());
    if !deferred_binding_still_applies(&state.svc.mux, &binding) {
        state
            .svc
            .deferred_session_bindings
            .clear_generation(&tab_id, generation);
        state.svc.emit_session_binding_state(&tab_id);
        log::info!(
            "[session-binding-deferral] dropping sid {} for Tab {}: current session moved from {} to {}",
            binding.session_id,
            tab_id,
            binding.previous_session_id,
            current_session.as_deref().unwrap_or("-"),
        );
        return;
    }
    if !binding.transcript_path.is_file() {
        return;
    }
    let evidence_session = binding.session_id.clone();
    let evidence_path = binding.transcript_path.to_string_lossy().into_owned();
    let evidence_recorded = tokio::task::spawn_blocking(move || {
        codeterm_plugin::session_evidence::record(&evidence_session, &evidence_path)
    })
    .await
    .unwrap_or(false);
    if !evidence_recorded {
        log::warn!(
            "[session-binding-deferral] could not record materialized transcript for sid {} Tab {}",
            binding.session_id,
            tab_id,
        );
        return;
    }
    match bind_reported_session(
        &state.svc.mux,
        state.svc.agent_registry.as_ref(),
        &tab_id,
        &binding.provider_id,
        &binding.session_id,
        report_nonce.as_deref(),
    ) {
        Ok(claim) => {
            let _ = finish_session_binding(
                &state,
                &tab_id,
                &binding.provider_id,
                &binding.session_id,
                report_nonce.as_deref(),
                &source,
                &body,
                claim,
            )
            .await;
        }
        Err((_, message, deferrable)) => {
            if deferrable.is_none() {
                state
                    .svc
                    .deferred_session_bindings
                    .refuse(
                        &tab_id,
                        &binding.session_id,
                        Some(&binding.previous_session_id),
                        &message,
                    );
                state.svc.emit_session_binding_state(&tab_id);
            }
            log::warn!(
                "[session-binding-deferral] sid {} for Tab {} did not bind: {}",
                binding.session_id,
                tab_id,
                message,
            );
        }
    }
}

pub(crate) fn deferred_binding_still_applies(
    mux: &crate::multiplexer::commands::MuxState,
    binding: &crate::service::session_binding_deferral::DeferredSessionBinding,
) -> bool {
    mux.read()
        .get_tab(&binding.tab_id)
        .and_then(|tab| tab.agent_session_id.as_deref())
        == Some(binding.previous_session_id.as_str())
}

pub(crate) async fn finish_session_binding(
    state: &AppState,
    tab_id: &str,
    provider_id: &str,
    sid: &str,
    report_nonce: Option<&str>,
    source: &str,
    body: &serde_json::Value,
    claim: SessionStartClaim,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if claim.changed {
        state.svc.deferred_session_bindings.clear(tab_id);
    } else {
        state
            .svc
            .deferred_session_bindings
            .clear_session(tab_id, sid);
    }
    state.svc.emit_session_binding_state(tab_id);
    let (current_nonce, reported_nonce) = {
        let mux = state.svc.mux.read();
        let tab = mux.get_tab(&tab_id);
        (
            tab.and_then(|tab| tab.session_binding_nonce.clone()),
            tab.and_then(|tab| tab.reported_session_binding_nonce.clone()),
        )
    };
    log::info!(
        "[delivery-timeline] event=session-binding tab={} provider={} session={} \
         report_nonce={} current_nonce={} reported_nonce={} nonce_verified={} changed={}",
        tab_id,
        provider_id,
        sid,
        report_nonce.unwrap_or("-"),
        current_nonce.as_deref().unwrap_or("-"),
        reported_nonce.as_deref().unwrap_or("-"),
        claim.nonce_verified,
        claim.changed,
    );
    // Authoritative identity commit: journaled only on a nonce-proven binding,
    // so an unverified report can never become restorable authority.
    if let (true, Some(binding_nonce)) = (claim.nonce_verified, reported_nonce.clone()) {
        state.svc.record_lifecycle(
            &tab_id,
            crate::session::lifecycle::LifecycleTransition::ProviderIdentityCommitted {
                provider: provider_id.to_string(),
            },
        );
        state.svc.record_lifecycle(
            &tab_id,
            crate::session::lifecycle::LifecycleTransition::SessionBound {
                session_id: sid.to_string(),
                binding_nonce,
            },
        );
    }
    crate::service::general_agent::binding_wait::notify_session_bound();
    acknowledge_authenticated_launch(&state.svc.mux, tab_id, &claim, report_nonce);
    if claim.nonce_verified {
        state.svc.rearm_authenticated_agent_input(&tab_id);
    }

    // while it runs — not only after completion.
    propagate_run_session(state, tab_id, sid);
    if let Some(reported_transcript) = reported_transcript_path(body) {
        let evidence_sid = sid.to_string();
        let persisted = tokio::task::spawn_blocking(move || {
            codeterm_plugin::session_evidence::record(&evidence_sid, &reported_transcript)
        })
        .await
        .unwrap_or(false);
        if !persisted {
            log::warn!(
                "[hook_session_start] tab={} could not persist reported transcript_path for {}",
                tab_id,
                sid
            );
        }
    }
    let transcript_progress = durable_transcript_progress(state, tab_id, provider_id, sid).await;
    state
        .svc
        .run_watcher
        .observe_durable_transcript(&state.svc, &tab_id, transcript_progress);

    if !claim.changed {
        if claim.nonce_verified {
            state.svc.bind_planner_session(&tab_id);
            state.svc.schedule_planner_replay(&tab_id);
        }
        return Ok(Json(serde_json::json!({"ok": true, "changed": false})));
    }

    log::info!(
        "[hook_session_start] tab={} provider={} source={} session {:?} -> {}",
        tab_id,
        provider_id,
        source,
        claim.prev.as_deref().unwrap_or("-"),
        sid
    );
    if let Ok(Some(resume)) =
        state
            .svc
            .plan_store
            .observe_handoff_session_start(tab_id, sid, epoch_ms())
    {
        state.svc.send_text_detached(&resume.tab_id, &resume.text);
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
    state.svc.chat_registry.shutdown_tab(&tab_id);
    state.svc.bind_planner_session(&tab_id);
    state.svc.schedule_planner_replay(&tab_id);
    let effort_cwd = state
        .svc
        .mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| tab.cwd.clone());
    let persist_svc = state.svc.clone();
    let effort_tab_id = tab_id.to_string();
    let effort_provider_id = provider_id.to_string();
    let effort_session_id = sid.to_string();
    if let Ok(Err(error)) = tokio::task::spawn_blocking(move || {
        if let Some(cwd) = effort_cwd {
            persist_svc.refresh_session_reasoning_effort_once(
                &effort_tab_id,
                &effort_provider_id,
                &cwd,
                &effort_session_id,
            );
        }
        crate::service::session_persist::persist_session_after_mutation(
            &persist_svc,
            crate::service::session_persist::SessionPersistUrgency::Prompt,
        )
    })
    .await
    {
        log::warn!("[context-epoch] failed to persist reset boundary: {error}");
    }
    state.svc.emit_state_changed();
    Ok(Json(serde_json::json!({"ok": true, "changed": true})))
}

async fn durable_transcript_progress(
    state: &AppState,
    tab_id: &str,
    provider_id: &str,
    session_id: &str,
) -> bool {
    let cwd = state
        .svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.cwd.clone())
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
    tab_id: &str,
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
        mux.get_tab(tab_id).and_then(|tab| {
            (tab.reported_session_binding_nonce.as_deref() == Some(binding_nonce))
                .then(|| tab.pty_id.clone())
                .flatten()
                .map(|pty_id| crate::service::ProviderSessionBinding {
                    pty_id,
                    binding_nonce: binding_nonce.to_string(),
                })
        })
    };
    binding.is_some_and(|binding| crate::service::record_provider_session(tab_id, &binding))
}

/// A report carrying a binding nonce that disagrees with the tab's own launch
/// nonce is a foreign or stale reporter: reject it so it affects no tab. A
/// report with no nonce keeps the pre-existing tolerance.
fn reject_foreign_binding_nonce(
    state: &AppState,
    tab_id: &str,
    headers: &HeaderMap,
    body: &serde_json::Value,
) -> Result<(), (StatusCode, String)> {
    let reported = crate::agent::permission_request::body_binding_nonce(body)
        .or_else(|| header_str(headers, "x-codeterm-binding-nonce"))
        .map(str::trim)
        .filter(|nonce| !nonce.is_empty());
    let Some(reported) = reported else {
        return Ok(());
    };
    if binding_nonce_scoped(&state.svc.mux, tab_id, reported) {
        return Ok(());
    }
    log::warn!("[hook] rejecting report for Tab {tab_id}: binding nonce mismatch");
    Err((StatusCode::CONFLICT, "binding nonce mismatch".to_string()))
}

/// True when the reported nonce matches the tab's launch nonce, or the tab has
/// no launch nonce to compare against.
fn binding_nonce_scoped(
    mux: &crate::multiplexer::commands::MuxState,
    tab_id: &str,
    reported: &str,
) -> bool {
    match mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.session_binding_nonce.clone())
    {
        Some(expected) => expected == reported,
        None => true,
    }
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

/// Provider-reported transcript path riding the SessionStart body. Persisted
/// only after the report was accepted for this tab's own session above.
fn reported_transcript_path(body: &serde_json::Value) -> Option<String> {
    session_start_string(body, &["transcript_path"])
        .or_else(|| session_start_string(body, &["payload", "transcript_path"]))
}

/// Push the just-bound session id (+cwd) onto whatever hidden run owns this tab
/// (mem worker or cron run), so the read-only chat-ui can render it live.
fn propagate_run_session(state: &AppState, tab_id: &str, sid: &str) {
    use tauri::Manager;

    let (worker_id, cwd) = {
        let mux = state.svc.mux.write();
        let tab = mux.get_tab(tab_id);
        (
            tab.and_then(|p| p.worker_id.clone()),
            tab.and_then(|p| p.cwd.clone()),
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
            .get_tab_owner(tab_id)
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
/// tab fields (`cwd`, current `agent_session_id`) snapshotted just before.
struct MaterializationProbe {
    cwd: String,
    prev_sid: Option<String>,
    new_materialized: bool,
    prev_materialized: bool,
}

pub(crate) fn bind_reported_session(
    mux: &crate::multiplexer::commands::MuxState,
    registry: &crate::agent::registry::AgentRegistry,
    tab_id: &str,
    provider_id: &str,
    sid: &str,
    report_nonce: Option<&str>,
) -> Result<SessionStartClaim, SessionBindError> {
    if registry.with_provider(provider_id, |_| ()).is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("unknown provider_id '{}'", provider_id),
            None,
        ));
    }

    // `session_exists` scans a QuickJS-backed store; under the mux lock that freezes every tab mutation.
    // Snapshot probe inputs, probe unlocked, commit only if the snapshot still holds — a moved binding retries, never overwrites.
    const MAX_ATTEMPTS: usize = 4;
    for _ in 0..MAX_ATTEMPTS {
        let (probe_cwd, probe_prev) = {
            let mux = mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return Err((StatusCode::NOT_FOUND, "Tab not found".to_string(), None));
            };
            (
                tab.cwd.clone().unwrap_or_default(),
                tab.agent_session_id.clone(),
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
        match commit_reported_session(&mut mux, tab_id, provider_id, sid, report_nonce, &probe)? {
            Some(claim) => return Ok(claim),
            None => continue, // snapshot went stale under us — reprobe.
        }
    }

    Err((
        StatusCode::CONFLICT,
        "session binding raced concurrent mutation; retry".to_string(),
        None,
    ))
}

/// Reacquire-and-commit half of [`bind_reported_session`]. Runs under the write
/// lock with the out-of-lock probe results. Returns `Ok(None)` when the probe
/// snapshot no longer matches live tab state, signalling the caller to reprobe.
fn commit_reported_session(
    mux: &mut crate::multiplexer::state::MultiplexerState,
    tab_id: &str,
    provider_id: &str,
    sid: &str,
    report_nonce: Option<&str>,
    probe: &MaterializationProbe,
) -> Result<Option<SessionStartClaim>, SessionBindError> {
    let Some(tab) = mux.get_tab(tab_id) else {
        return Err((StatusCode::NOT_FOUND, "Tab not found".to_string(), None));
    };
    // The probe keyed on these; if either moved, the probe is stale — reprobe.
    // This is what stops a stale snapshot from overwriting a changed binding.
    if tab.cwd.clone().unwrap_or_default() != probe.cwd || tab.agent_session_id != probe.prev_sid {
        return Ok(None);
    }

    // A report carrying a nonce that disagrees with the tab's launch nonce is a stale or foreign reporter (relaunch, /new in another process, reused tab id). A matching nonce is proof we launched this session ourselves — it's authoritative even before the session file lands on disk. A missing report nonce is tolerated only after provider-owned session validation below.
    let launch_nonce = tab.session_binding_nonce.clone();
    let acknowledged_nonce = tab.reported_session_binding_nonce.clone();
    let nonce_verified = if let Some(expected) = tab.session_binding_nonce.clone() {
        match report_nonce {
            Some(got) if got != expected => {
                log::warn!(
                    "[hook_session_start] rejecting sid {} for Tab {} provider={}: binding nonce mismatch",
                    sid, tab_id, provider_id,
                );
                return Err((
                    StatusCode::CONFLICT,
                    "binding nonce mismatch".to_string(),
                    None,
                ));
            }
            Some(_) => true,
            None => false,
        }
    } else {
        false
    };

    if let Some(existing_provider) = tab.agent_type.as_deref() {
        if existing_provider != provider_id {
            return Err((
                StatusCode::CONFLICT,
                format!(
                    "Tab provider '{}' does not match report provider '{}'",
                    existing_provider, provider_id
                ),
                None,
            ));
        }
    }

    let exists = nonce_verified || probe.new_materialized;
    if !exists {
        log::warn!(
            "[hook_session_start] rejecting sid {} for Tab {} provider={}: session does not belong to provider",
            sid,
            tab_id,
            provider_id,
        );
        return Err((
            StatusCode::CONFLICT,
            format!(
                "session_id '{}' is not a known session for provider '{}'",
                sid, provider_id
            ),
            None,
        ));
    }

    let conflict = mux
        .tab_ids_with_session(sid)
        .into_iter()
        .find(|other| other != tab_id);
    if let Some(other) = conflict {
        log::warn!(
            "[hook_session_start] rejecting sid {} for Tab {} provider={} already claimed by {}",
            sid,
            tab_id,
            provider_id,
            other,
        );
        return Err((
            StatusCode::CONFLICT,
            format!("session_id already claimed by Tab {}", other),
            None,
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
                "[hook_session_start] rejecting fileless sid {} for Tab {} provider={}: would evict materialized session {:?}",
                sid, tab_id, provider_id, probe.prev_sid,
            );
            return Err((
                StatusCode::CONFLICT,
                format!(
                    "session_id '{}' cannot evict materialized session {:?}",
                    sid, probe.prev_sid
                ),
                probe
                    .prev_sid
                    .clone()
                    .map(|previous_session_id| DeferredBindingPremise {
                        previous_session_id,
                    }),
            ));
        }
    }

    let Some(tab) = mux.get_tab_mut(tab_id) else {
        return Err((StatusCode::NOT_FOUND, "Tab not found".to_string(), None));
    };
    if tab.agent_type.is_none() {
        tab.agent_type = Some(provider_id.to_string());
        tab.agent_type_locked = true;
        tab.agent_set_at = Some(std::time::Instant::now());
    }

    let prev = tab.agent_session_id.clone();
    let changed = prev.as_deref() != Some(sid);
    if changed {
        if let Some(previous_session_id) = prev.as_deref() {
            tab.archive_context_epoch(provider_id, previous_session_id);
        }
        tab.agent_session_id = Some(sid.to_string());
        crate::service::tab::clear_runtime_model_evidence(tab);
    }
    tab.reported_session_binding_nonce =
        nonce_verified.then(|| report_nonce.unwrap_or_default().to_string());
    Ok(Some(SessionStartClaim {
        prev,
        changed,
        nonce_verified,
    }))
}

#[cfg(test)]
mod tests;
