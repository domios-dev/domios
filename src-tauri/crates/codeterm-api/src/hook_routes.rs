use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use std::collections::BTreeMap;

use crate::routes::{check_auth, resolve_tab, AppState};
use codeterm_service_core::agent::provider::{HookEventBinding, HookIngestionRoute};
use codeterm_service_core::agent::registry::AgentRegistry;
use codeterm_service_core::api_support::OptionalJson;

pub(crate) mod context_feed;
mod hook_authority;
pub(crate) mod permission_decision;
use hook_authority::{stop_report_verdict, StopReportVerdict};
mod provider_events;
mod reconciliation;
mod recording;
mod stop;
#[cfg(test)]
use codeterm_service_core::service::hook_session_binding::{
    acknowledge_and_reconcile_session_binding, acknowledge_authenticated_launch,
    bind_detected_session, session_start_string,
};
use codeterm_service_core::service::hook_session_binding::{
    bind_planner_session_after_claim, bind_reported_session, escaped_hook_log_field,
    finish_session_binding, reported_transcript_path, session_start_session_id,
    unchanged_acknowledged_hook_report, SessionStartClaim,
};
pub(crate) use provider_events::hook_provider_event;
use recording::record_hook_post;
pub use stop::hook_stop;
#[cfg(test)]
#[path = "hook_routes/prompt_resolution_tests.rs"]
mod prompt_resolution_tests;
#[cfg(test)]
#[path = "hook_routes/review_regressions.rs"]
mod review_regressions;
#[cfg(test)]
#[path = "hook_routes/surviving_shell_tests.rs"]
mod surviving_shell_tests;

async fn resolve_hook_tab_async(
    state: &AppState,
    raw_tab_id: &str,
    event: &str,
) -> Result<String, (StatusCode, String)> {
    let state = state.clone();
    let raw_tab_id = raw_tab_id.to_owned();
    let event = event.to_owned();
    tokio::task::spawn_blocking(move || resolve_hook_tab(&state, &raw_tab_id, &event))
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

fn resolve_hook_tab(
    state: &AppState,
    raw_tab_id: &str,
    event: &str,
) -> Result<String, (StatusCode, String)> {
    match resolve_tab(state, raw_tab_id) {
        Ok(tab_id) => Ok(tab_id),
        Err((status, message)) => {
            if status == StatusCode::NOT_FOUND {
                let log_tab_id = escaped_hook_log_field(raw_tab_id);
                log::warn!(
                    "[hook-refusal] tab={} event={} reason=unknown_tab",
                    log_tab_id,
                    event
                );
            }
            Err((status, message.to_string()))
        }
    }
}

pub async fn release_launch_artifacts_api(
    AxumState(state): AxumState<AppState>,
    Path(raw_tab_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    let tab_id = crate::routes::resolve_tab_async(&state, &raw_tab_id)
        .await
        .map_err(|(status, message)| (status, message.to_string()))?;
    let owner = state.clone();
    let released = tokio::task::spawn_blocking(move || {
        let state = owner;
        let provider_id = state
            .svc
            .mux
            .read()
            .get_tab(&tab_id)
            .and_then(|tab| tab.agent_type.clone());
        let Some(provider_id) = provider_id else {
            return Ok(Json(serde_json::json!({ "ok": true, "released": false })));
        };
        let registry = state.svc.agent_registry.clone();
        let released = codeterm_service_core::service::launch_artifacts::release_tab(
            &registry,
            &tab_id,
            &provider_id,
        )
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
        Ok(Json(
            serde_json::json!({ "ok": true, "released": released }),
        ))
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed".to_string(),
        )
    })?;
    released
}

fn can_short_circuit_session_start(
    claim: &SessionStartClaim,
    fallback_pending: bool,
    fallback_retry_available: bool,
) -> bool {
    claim.is_unchanged_acknowledged_report()
        && !(fallback_pending && claim.nonce_verified && fallback_retry_available)
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
pub(crate) fn foreign_provider_session_owner<'a>(
    tab_provider: Option<&'a str>,
    reported_provider: &str,
    live_owner: bool,
) -> Option<&'a str> {
    tab_provider.filter(|owner| live_owner && *owner != reported_provider)
}

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
                codeterm_service_core::agent::provider_identity::provider_for_claimed_session(
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
        codeterm_service_core::session::lifecycle::AgentTypeWriteOrigin::Hook,
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
    let tab_id =
        crate::hook_routes::resolve_hook_tab_async(&state, &raw_tab_id, "permission").await?;
    record_hook_post(&state, &tab_id, "permission", body.as_ref());
    if let Some(response) = hook_authority::telemetry_response(&tab_id, "permission") {
        return Ok(response);
    }
    if let Some(raw) = body.as_ref() {
        if let Some(response) = permission_decision::handle(&state, &tab_id, &headers, raw).await {
            return Ok(Json(response));
        }
    }
    tokio::task::spawn_blocking(move || {
        // Hook firing = authoritative proof that an agent is running in this tab.
        ensure_agent_state(&state, &tab_id);

        // The provider's request metadata is retained separately from screen choices.
        let permission_request = body
            .as_ref()
            .and_then(codeterm_service_core::agent::permission_request::parse_permission_request);
        let has_permission_request = permission_request.is_some();
        if let (Some(signal), Some(raw)) = (permission_request, body.as_ref()) {
            reject_foreign_binding_nonce(&state, &tab_id, &headers, raw)?;
            state
                .svc
                .prompt_detector
                .lock()
                .hook_detected_permission_request(&tab_id, signal);
        }

        let explicit_prompt = if has_permission_request {
            None
        } else {
            body.and_then(|b| {
                let question = b.get("question")?.as_str()?.to_string();
                let options = b
                    .get("options")?
                    .as_array()?
                    .iter()
                    .filter_map(|o| {
                        Some(codeterm_service_core::multiplexer::types::PromptOption {
                            label: o.get("label")?.as_str()?.to_string(),
                            value: o.get("value")?.as_str()?.to_string(),
                        })
                    })
                    .collect::<Vec<_>>();
                if options.is_empty() {
                    return None;
                }
                Some(codeterm_service_core::multiplexer::types::PendingPrompt {
                    request_id: None,
                    question,
                    options,
                    sections: None,
                    multi_select: None,
                    submit_available: None,
                    selected_values: None,
                })
            })
        };

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
                let at = agent_type
                    .unwrap_or_else(|| state.svc.configuration().general_agent.agent_provider);
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

                let parsed = codeterm_service_core::agent::prompt::parse_prompt_safely(|| {
                    registry.parse_prompt(&at, &clean)
                });
                state.svc.prompt_detector.lock().hook_detected_parsed(
                    &tab_id,
                    clean.len(),
                    None,
                    parsed,
                )
            } else {
                codeterm_service_core::agent::prompt::PromptDecision::None
            }
        };

        state.svc.apply_hook_prompt_decision(&tab_id, &decision);

        let response = if has_permission_request {
            serde_json::json!({"ok": true, "ingested": true})
        } else {
            serde_json::json!({"ok": true})
        };
        Ok(Json(response))
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed".to_string(),
        )
    })?
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
) -> Result<axum::response::Response, (StatusCode, String)> {
    let owner = state.clone();
    let prepared = tokio::task::spawn_blocking(move || {
        let state = owner;
    let tab_id = resolve_hook_tab(&state, &raw_tab_id, "session_start")?;
    if let Some(response) = hook_authority::telemetry_response(&tab_id, "session_start") {
        return Ok(Err(axum::response::IntoResponse::into_response(response)));
    }
    let body = body.unwrap_or(serde_json::Value::Null);
    record_hook_post(&state, &tab_id, "session_start", Some(&body));
    let session_id = session_start_session_id(&body);
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
    if marked_hook_missing_binding_nonce(&headers, report_nonce.as_deref()) {
        return Err((
            StatusCode::CONFLICT,
            "CodeTerm session hook is missing its binding nonce".into(),
        ));
    }
    log::debug!(
        "[hook_session_start] received tab={} source={} session_id_present={} nonce_present={}",
        tab_id,
        source,
        session_id.is_some(),
        report_nonce.is_some(),
    );

    let Some(sid) = session_id else {
        log::warn!(
            "[hook_session_start] tab={} source={} body had no session_id",
            tab_id,
            source
        );
        state.svc.emit_session_binding_state(&tab_id);
        return Ok(Err(axum::response::IntoResponse::into_response(Json(
            serde_json::json!({"ok": false, "reason": "no session_id in body"}),
        ))));
    };

    let (existing, cwd, current_session_id, live_owner) = state
        .svc
        .mux
        .read()
        .get_tab(&tab_id)
        .map(|tab| {
            (
                tab.agent_type.clone(),
                tab.cwd.clone(),
                tab.agent_session_id.clone(),
                codeterm_service_core::service::tab_has_live_agent(tab),
            )
        })
        .unwrap_or_default();
    let Some(provider_id) = session_start_provider(
        &state.svc.agent_registry,
        provider_id,
        existing.clone(),
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
    if let Some(owner) =
        foreign_provider_session_owner(existing.as_deref(), &provider_id, live_owner)
    {
        // A child process started from this tab's shell inherits CODETERM_TAB_ID; it is not the tab's agent.
        let message = format!(
            "session_id '{sid}' belongs to provider {provider_id}, but Tab {tab_id} runs a verified {owner} agent"
        );
        log::warn!("[hook_session_start] ignoring foreign-provider session: {message}");
        return Err((StatusCode::CONFLICT, message));
    }

    let mut fallback_retry = None;
    let claim = match bind_reported_session(
        &state.svc.mux,
        state.svc.agent_registry.as_ref(),
        &tab_id,
        &provider_id,
        &sid,
        report_nonce.as_deref(),
    ) {
        Ok(claim) => {
            let fallback_pending =
                codeterm_service_core::service::integration_silence::has_hook_binding_fallback(
                    &provider_id,
                    &sid,
                );
            let fallback_retry_available = if claim.is_unchanged_acknowledged_report()
                && fallback_pending
            {
                fallback_retry =
                        codeterm_service_core::service::integration_silence::reserve_hook_binding_fallback_retry(
                            &provider_id,
                            &sid,
                        );
                fallback_retry.is_some()
            } else {
                false
            };
            if can_short_circuit_session_start(&claim, fallback_pending, fallback_retry_available) {
                return Ok(Ok((true, claim, fallback_retry, tab_id, provider_id, sid, report_nonce, source, body)));

            }
            claim
        }
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
            return Ok(Err(axum::response::IntoResponse::into_response(Json(
                serde_json::json!({
                    "ok": true,
                    "changed": false,
                    "deferred": true,
                    "session_id": sid,
                    "current_session_id": premise.previous_session_id,
                }),
            ))));
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
        Ok::<_, (StatusCode, String)>(Ok((false, claim, fallback_retry, tab_id, provider_id, sid, report_nonce, source, body)))
    }).await.map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    let (
        duplicate,
        claim,
        mut fallback_retry,
        tab_id,
        provider_id,
        sid,
        report_nonce,
        source,
        body,
    ) = match prepared {
        Ok(prepared) => prepared,
        Err(response) => return Ok(response),
    };
    if duplicate {
        bind_planner_session_after_claim(&state.svc, &tab_id).await?;
        let owner = state.clone();
        return tokio::task::spawn_blocking(move || {
            let response = unchanged_acknowledged_hook_report(
                &owner.svc,
                &tab_id,
                &provider_id,
                &sid,
                &source,
                &claim,
                report_nonce.as_deref(),
            );
            reconciliation::after_response(response, owner.svc, tab_id, "session_start_duplicate")
        })
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()));
    }
    let result = finish_session_binding(
        &state.svc,
        &tab_id,
        &provider_id,
        &sid,
        report_nonce.as_deref(),
        Some(codeterm_service_core::session::lifecycle::AgentTypeWriteOrigin::Hook),
        &source,
        &body,
        claim,
        codeterm_service_core::session::lifecycle::BindingSource::Hook,
        true,
    )
    .await;
    if result.is_ok() {
        if let Some(retry) = fallback_retry.take() {
            retry.commit();
        }
    }
    result.map(axum::response::IntoResponse::into_response)
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
    let owner = state.clone();
    let target = tab_id.clone();
    let binding = tokio::task::spawn_blocking(move || {
        let state = owner;
        let tab_id = target;
    let Some(binding) = state.svc.deferred_session_bindings.get(&tab_id, generation) else {
        return None;
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
        return None;
    }
    if !binding.transcript_path.is_file() {
        return None;
    }
        Some(binding)
    }).await.ok().flatten();
    let Some(binding) = binding else {
        return;
    };
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
    let owner = state.clone();
    let target = tab_id.clone();
    let provider = binding.provider_id.clone();
    let session = binding.session_id.clone();
    let nonce = report_nonce.clone();
    let claim = tokio::task::spawn_blocking(move || {
        bind_reported_session(
            &owner.svc.mux,
            state.svc.agent_registry.as_ref(),
            &target,
            &provider,
            &session,
            nonce.as_deref(),
        )
    })
    .await;
    let Ok(claim) = claim else {
        return;
    };
    match claim {
        Ok(claim) => {
            let _ = finish_session_binding(
                &state.svc,
                &tab_id,
                &binding.provider_id,
                &binding.session_id,
                report_nonce.as_deref(),
                Some(codeterm_service_core::session::lifecycle::AgentTypeWriteOrigin::Hook),
                &source,
                &body,
                claim,
                codeterm_service_core::session::lifecycle::BindingSource::Hook,
                true,
            )
            .await;
        }
        Err((_, message, deferrable)) => {
            let _ = tokio::task::spawn_blocking(move || {
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
            })
            .await;
        }
    }
}

pub(crate) fn deferred_binding_still_applies(
    mux: &codeterm_service_core::multiplexer::commands::MuxState,
    binding: &codeterm_service_core::service::session_binding_deferral::DeferredSessionBinding,
) -> bool {
    mux.read()
        .get_tab(&binding.tab_id)
        .and_then(|tab| tab.agent_session_id.as_deref())
        == Some(binding.previous_session_id.as_str())
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
    let reported = codeterm_service_core::agent::permission_request::body_binding_nonce(body)
        .or_else(|| header_str(headers, "x-codeterm-binding-nonce"))
        .map(str::trim)
        .filter(|nonce| !nonce.is_empty());
    let Some(reported) = reported else {
        return Ok(());
    };
    if binding_nonce_scoped(&state.svc.mux, tab_id, reported) {
        return Ok(());
    }
    log::warn!("[hook-refusal] tab={tab_id} event=permission reason=nonce_mismatch");
    Err((StatusCode::CONFLICT, "binding nonce mismatch".to_string()))
}

/// True when the reported nonce matches the tab's launch nonce, or the tab has
/// no launch nonce to compare against.
fn binding_nonce_scoped(
    mux: &codeterm_service_core::multiplexer::commands::MuxState,
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

fn marked_hook_missing_binding_nonce(headers: &HeaderMap, report_nonce: Option<&str>) -> bool {
    headers.contains_key("x-codeterm-hook")
        && !report_nonce.is_some_and(|nonce| !nonce.trim().is_empty())
}

#[cfg(test)]
mod tests;
