use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::agent::provider::{HookEventBinding, HookIngestionRoute};
use crate::agent::registry::AgentRegistry;
use crate::api::{
    optional_json::OptionalJson,
    routes::{resolve_tab, AppState},
};

mod run_session;
mod session_claim;

#[cfg(test)]
pub(crate) use session_claim::bind_validated_rebind_session;
pub(crate) use session_claim::{bind_reported_session, commit_materialized_rebind_session};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionStartClaim {
    prev: Option<String>,
    changed: bool,
    nonce_verified: bool,
    identity_evidence: crate::agent::provider_identity::ProviderIdentityEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeferredBindingPremise {
    previous_session_id: String,
}

pub(crate) type SessionBindError = (StatusCode, String, Option<DeferredBindingPremise>);

#[derive(Clone, Copy, PartialEq, Eq)]
enum SessionBindingSource {
    Hook,
    ValidatedRebind,
}

impl SessionStartClaim {
    pub(crate) fn identity_evidence(
        &self,
    ) -> &crate::agent::provider_identity::ProviderIdentityEvidence {
        &self.identity_evidence
    }
}

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

/// SessionStart identity ladder: report > tab > the store owning the reported
/// session > the route's sole declarer. Unknown stays unknown, never a default.
fn session_start_provider(
    registry: &AgentRegistry,
    reported: Option<String>,
    tab_provider: Option<String>,
    cwd: Option<&str>,
    session_id: &str,
) -> Option<String> {
    reported
        .or(tab_provider)
        .or_else(|| {
            cwd.and_then(|cwd| {
                crate::agent::provider_identity::provider_for_claimed_session(
                    registry, cwd, session_id,
                )
            })
        })
        .or_else(|| sole_hook_route_declarer(registry, HookIngestionRoute::SessionStart))
}

/// Attribute an unclaimed shell from the provider-specific permission route.
/// A claimed session waits for SessionStart to prove its provider.
fn ensure_agent_state(state: &AppState, tab_id: &str) {
    log::info!("[ensure_agent_state] called for Tab {}", tab_id);
    let (agent_type, session_id) = {
        let mux = state.svc.mux.read();
        let tab = match mux.get_tab(tab_id) {
            Some(p) => p,
            None => return,
        };
        (tab.agent_type.clone(), tab.agent_session_id.clone())
    };
    if agent_type.is_some() {
        return;
    }
    if session_id.is_some() {
        log::warn!(
            "[hook] Tab {} fired the permission route with a claimed session; agent_type left unset until SessionStart proves ownership",
            tab_id
        );
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
    state.svc.set_and_lock_tab_agent_type_from(
        tab_id,
        owner.clone(),
        crate::session::lifecycle::AgentTypeWriteOrigin::Hook,
        Some(tab_id),
    );
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
    let body = body.unwrap_or(serde_json::Value::Null);
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

    // A nonce-verified report is the completion-via-report path; a nonce-less legacy post keeps today's tab-keyed behavior.
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
    state.svc.on_agent_turn_settled(&tab_id);
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

    let (existing, cwd, current_session_id) = state
        .svc
        .mux
        .read()
        .get_tab(&tab_id)
        .map(|tab| {
            (
                tab.agent_type.clone(),
                tab.cwd.clone(),
                tab.agent_session_id.clone(),
            )
        })
        .unwrap_or_default();
    let Some(provider_id) = session_start_provider(
        &state.svc.agent_registry,
        provider_id,
        existing,
        cwd.as_deref(),
        &sid,
    ) else {
        let message = format!("session_id '{sid}' has no provider owner for Tab {tab_id}");
        log::warn!(
            "[hook_session_start] rejecting sid {sid} for Tab {tab_id}: provider unresolved"
        );
        state.svc.deferred_session_bindings.refuse(
            &tab_id,
            &sid,
            current_session_id.as_deref(),
            &message,
        );
        state.svc.emit_session_binding_state(&tab_id);
        return Err((StatusCode::CONFLICT, message));
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
                state.svc.deferred_session_bindings.refuse(
                    &tab_id,
                    &sid,
                    Some(&premise.previous_session_id),
                    &message,
                );
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
                state.svc.deferred_session_bindings.refuse(
                    &tab_id,
                    &sid,
                    Some(&premise.previous_session_id),
                    &reason,
                );
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
            state.svc.deferred_session_bindings.refuse(
                &tab_id,
                &sid,
                current_session_id.as_deref(),
                &message,
            );
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
        Some(crate::session::lifecycle::AgentTypeWriteOrigin::Hook),
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
                Some(crate::session::lifecycle::AgentTypeWriteOrigin::Hook),
                &source,
                &body,
                claim,
            )
            .await;
        }
        Err((_, message, deferrable)) => {
            if deferrable.is_none() {
                state.svc.deferred_session_bindings.refuse(
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
    identity_origin: Option<crate::session::lifecycle::AgentTypeWriteOrigin>,
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
    let identity_committed = if let Some(origin) = identity_origin {
        if origin == crate::session::lifecycle::AgentTypeWriteOrigin::Hook {
            matches!(
                state.svc.write_hook_tab_agent_type_from_claim(
                    tab_id,
                    provider_id.to_string(),
                    Some(tab_id),
                    claim.identity_evidence(),
                ),
                Some(
                    crate::service::provider_identity::AgentTypeWriteOutcome::Applied { .. }
                        | crate::service::provider_identity::AgentTypeWriteOutcome::Kept { .. }
                )
            )
        } else {
            state.svc.set_and_lock_tab_agent_type_from(
                tab_id,
                provider_id.to_string(),
                origin,
                Some(tab_id),
            );
            true
        }
    } else {
        false
    };
    // Authoritative identity commit: journaled only on a nonce-proven binding,
    // so an unverified report can never become restorable authority.
    if let (true, true, Some(binding_nonce)) = (
        identity_committed,
        claim.nonce_verified,
        reported_nonce.clone(),
    ) {
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
    let launch_identity =
        authenticated_launch_identity(&state.svc.mux, tab_id, &claim, report_nonce);
    acknowledge_authenticated_launch(&state.svc.mux, tab_id, &claim, report_nonce);
    if claim.nonce_verified {
        state.svc.rearm_authenticated_agent_input(&tab_id);
        state.svc.reconcile_boot_stage_with_session_binding(&tab_id);
    }
    // Ordering: the identity above is read from state the nonce check already
    // committed, so an unverified report reaches this with nothing to offer.
    if let Some(evidence) = launch_identity.as_ref() {
        state.svc.retire_provisional_launch(tab_id, evidence);
    }

    // while it runs — not only after completion.
    run_session::propagate(state, tab_id, sid);
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
    let persist_svc = state.svc.clone();
    let bound_tab_id = tab_id.to_string();
    if let Ok(Err(error)) = tokio::task::spawn_blocking(move || {
        // The commit cleared this tab's runtime model and effort evidence for
        // the retired session; the binding is the event that re-reads it.
        persist_svc.refresh_session_model_after_binding(&bound_tab_id);
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

/// The authoritative launch identity of a session-start report: the tab's own
/// live PTY paired with the launch nonce its agent echoed and the commit above
/// already verified against this tab's current launch.
fn authenticated_launch_identity(
    mux: &crate::multiplexer::commands::MuxState,
    tab_id: &str,
    claim: &SessionStartClaim,
    report_nonce: Option<&str>,
) -> Option<crate::service::ProviderSessionBinding> {
    if !claim.nonce_verified {
        return None;
    }
    let binding_nonce = report_nonce?;
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
}

fn acknowledge_authenticated_launch(
    mux: &crate::multiplexer::commands::MuxState,
    tab_id: &str,
    claim: &SessionStartClaim,
    report_nonce: Option<&str>,
) -> bool {
    authenticated_launch_identity(mux, tab_id, claim, report_nonce).is_some_and(|binding| {
        // The same proof the launch registry takes is this launch's readiness.
        crate::service::launch_readiness::note_acknowledged(
            tab_id,
            &binding.pty_id,
            &binding.binding_nonce,
        );
        crate::service::record_provider_session(tab_id, &binding)
    })
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

#[cfg(test)]
mod tests;
