//! Session binding shared by the hook routes and launch-session detection.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::StatusCode;
use axum::Json;

mod run_session;
mod session_ack;
mod session_claim;
mod session_start_payload;
pub use session_ack::acknowledge_and_reconcile_session_binding;
#[cfg(any(test, feature = "test-support"))]
pub use session_ack::acknowledge_authenticated_launch;
pub use session_ack::unchanged_acknowledged_hook_report;
pub use session_claim::bind_detected_session;
pub use session_claim::bind_reported_session;
#[cfg(any(test, feature = "test-support"))]
pub use session_claim::bind_validated_rebind_session;
pub use session_claim::commit_materialized_rebind_session;
pub use session_start_payload::reported_transcript_path;
pub use session_start_payload::session_start_session_id;
pub use session_start_payload::session_start_string;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStartClaim {
    pub prev: Option<String>,
    pub changed: bool,
    pub nonce_verified: bool,
    pub nonce_already_acknowledged: bool,
    pub identity_evidence: crate::agent::provider_identity::ProviderIdentityEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferredBindingPremise {
    pub previous_session_id: String,
}

pub type SessionBindError = (StatusCode, String, Option<DeferredBindingPremise>);

pub fn escaped_hook_log_field(value: &str) -> String {
    format!("{value:?}")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SessionBindingSource {
    Hook,
    ValidatedRebind,
    LaunchDetection,
}

impl SessionStartClaim {
    pub fn is_unchanged_acknowledged_report(&self) -> bool {
        !self.changed && self.nonce_verified && self.nonce_already_acknowledged
    }

    pub fn identity_evidence(&self) -> &crate::agent::provider_identity::ProviderIdentityEvidence {
        &self.identity_evidence
    }
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub async fn bind_planner_session_after_claim(
    svc: &crate::service::ServiceContext,
    tab_id: &str,
) -> Result<(), (StatusCode, String)> {
    let service = svc.clone();
    let bound_tab = tab_id.to_string();
    tokio::task::spawn_blocking(move || service.bind_planner_session(&bound_tab))
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

pub async fn finish_session_binding(
    svc: &crate::service::ServiceContext,
    tab_id: &str,
    provider_id: &str,
    sid: &str,
    report_nonce: Option<&str>,
    identity_origin: Option<crate::session::lifecycle::AgentTypeWriteOrigin>,
    source: &str,
    body: &serde_json::Value,
    claim: SessionStartClaim,
    binding_source: crate::session::lifecycle::BindingSource,
    session_binding_hook_succeeded: bool,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    bind_planner_session_after_claim(svc, tab_id).await?;
    if claim.changed {
        crate::service::provider_signals::refresh_auth(svc, tab_id);
        if let Some(previous_session_id) = claim.prev.as_deref() {
            crate::service::integration_silence::clear_hook_binding_fallback_session(
                provider_id,
                previous_session_id,
            );
        }
        svc.deferred_session_bindings.clear(tab_id);
    } else {
        svc.deferred_session_bindings.clear_session(tab_id, sid);
    }
    svc.emit_session_binding_state(tab_id);
    if claim.nonce_verified && (claim.changed || session_binding_hook_succeeded) {
        crate::service::integration_silence::record_hook_binding_source(
            svc.agent_registry.as_ref(),
            provider_id,
            sid,
            binding_source,
            session_binding_hook_succeeded,
        );
    }
    let (current_nonce, reported_nonce) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(&tab_id);
        (
            tab.and_then(|tab| tab.session_binding_nonce.clone()),
            tab.and_then(|tab| tab.reported_session_binding_nonce.clone()),
        )
    };
    log::info!(
        "[delivery-timeline] event=session-binding tab={} provider={} session={} source={} binding_source={} \
         report_nonce={} current_nonce={} reported_nonce={} nonce_verified={} changed={}",
        tab_id,
        provider_id,
        sid,
        source,
        crate::service::integration_silence::binding_source_label(binding_source),
        report_nonce.unwrap_or("-"),
        current_nonce.as_deref().unwrap_or("-"),
        reported_nonce.as_deref().unwrap_or("-"),
        claim.nonce_verified,
        claim.changed,
    );
    // A provider adopted from a surviving shell proves itself outside any launch.
    if claim.nonce_verified && svc.mux.read().lifecycle_key(tab_id).is_none() {
        svc.open_lifecycle_epoch(tab_id);
    }
    let identity_committed = if let Some(origin) = identity_origin {
        if origin == crate::session::lifecycle::AgentTypeWriteOrigin::Hook {
            matches!(
                svc.write_hook_tab_agent_type_from_claim(
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
            svc.set_and_lock_tab_agent_type_from(
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
    if let (true, true, Some(binding_nonce)) = (
        identity_committed,
        claim.nonce_verified,
        reported_nonce.clone(),
    ) {
        svc.record_lifecycle(
            &tab_id,
            crate::session::lifecycle::LifecycleTransition::ProviderIdentityCommitted {
                provider: provider_id.to_string(),
            },
        );
        svc.record_lifecycle(
            &tab_id,
            crate::session::lifecycle::LifecycleTransition::SessionBound {
                session_id: sid.to_string(),
                binding_nonce,
                source: binding_source,
            },
        );
    }
    let relay_service = svc.clone();
    let relay_tab = tab_id.to_string();
    drop(tokio::task::spawn_blocking(move || {
        relay_service.activate_bound_reply_relays(&relay_tab)
    }));
    crate::service::general_agent::binding_wait::notify_binding_state_changed();
    let launch_identity =
        acknowledge_and_reconcile_session_binding(svc, tab_id, &claim, report_nonce);
    if let Some(nonce) = report_nonce {
        let service = svc.clone();
        let tab = tab_id.to_string();
        let session = sid.to_string();
        let nonce = nonce.to_string();
        drop(tokio::task::spawn_blocking(move || {
            crate::service::public_send_readiness::observe_session_binding(
                &service, &tab, &session, &nonce,
            )
        }));
    }
    if let Some(evidence) = launch_identity.as_ref() {
        svc.retire_provisional_launch(tab_id, evidence);
    }
    // while it runs — not only after completion.
    let service = std::sync::Arc::new(svc.clone());
    let tab = tab_id.to_string();
    let session = sid.to_string();
    tokio::task::spawn_blocking(move || run_session::propagate(&service, &tab, &session))
        .await
        .map_err(|error| {
            log::error!(
                "[hook_session_start] tab={} run-session propagation failed: {}",
                tab_id,
                error
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to propagate session to cron run".to_string(),
            )
        })?;
    if let (true, Some(model)) = (claim.nonce_verified, session_start_string(body, &["model"])) {
        let (service, tab, provider) = (svc.clone(), tab_id.to_string(), provider_id.to_string());
        let session = sid.to_string();
        let _ = tokio::task::spawn_blocking(move || {
            service.record_launch_reported_model(&tab, &provider, &session, &model)
        })
        .await;
    }
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
    let transcript_progress = durable_transcript_progress(svc, tab_id, provider_id, sid).await;
    svc.run_watcher
        .observe_durable_transcript(svc, &tab_id, transcript_progress);

    if !claim.changed {
        if claim.nonce_verified {
            svc.schedule_planner_replay(&tab_id);
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
    if let Ok(Some(resume)) = svc
        .plan_store
        .observe_handoff_session_start(tab_id, sid, epoch_ms())
    {
        svc.send_text_detached(&resume.tab_id, &resume.text);
        let _ = svc.plan_store.mark_handoff_resume_sent(
            &resume.plan_id,
            &resume.handoff_id,
            epoch_ms(),
        );
        let _ = svc
            .plan_store
            .ack_handoff_resume(&resume.plan_id, &resume.handoff_id, epoch_ms());
        crate::events::emit_everywhere(
            &svc.app_handle,
            "plan-changed",
            serde_json::json!({ "plan_id": resume.plan_id }),
        );
    }
    if let Some(previous_session_id) = claim.prev.as_deref() {
        // Binding is already visible; Chat may have subscribed to the new session.
        svc.chat_registry
            .shutdown_session(tab_id, previous_session_id);
    }
    svc.schedule_planner_replay(&tab_id);
    let persist_svc = svc.clone();
    let bound_tab_id = tab_id.to_string();
    if let Ok(Err(error)) = tokio::task::spawn_blocking(move || {
        // The commit cleared this tab's runtime model and effort evidence for
        // the retired session; the binding is the event that re-reads it.
        persist_svc.refresh_session_model_for_current_tab(&bound_tab_id);
        crate::service::session_persist::persist_session_after_mutation(
            &persist_svc,
            crate::service::session_persist::SessionPersistUrgency::Prompt,
        )
    })
    .await
    {
        log::warn!("[context-epoch] failed to persist reset boundary: {error}");
    }
    svc.emit_state_changed();
    Ok(Json(serde_json::json!({"ok": true, "changed": true})))
}

async fn durable_transcript_progress(
    svc: &crate::service::ServiceContext,
    tab_id: &str,
    provider_id: &str,
    session_id: &str,
) -> bool {
    let cwd = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.cwd.clone())
        .unwrap_or_default();
    let registry = svc.agent_registry.clone();
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
