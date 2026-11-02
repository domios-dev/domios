use axum::http::StatusCode;

use super::{commit_materialized_rebind_session, escaped_hook_log_field, finish_session_binding};
use crate::agent::provider_identity::{provider_for_claimed_session, ProviderIdentityEvidence};
use crate::service::ServiceContext;

pub async fn recover_verified_hook_binding(
    svc: &ServiceContext,
    tab_id: &str,
    reported_provider: Option<&str>,
    reported_session: Option<&str>,
    report_nonce: Option<&str>,
) -> Result<(), (StatusCode, String)> {
    let Some(nonce) = report_nonce.filter(|nonce| !nonce.is_empty()) else {
        return Ok(());
    };
    let service = svc.clone();
    let target = tab_id.to_string();
    let provider = reported_provider.map(str::to_owned);
    let session = reported_session.map(str::to_owned);
    let nonce = nonce.to_string();
    let prepared = tokio::task::spawn_blocking(move || {
        let identity = {
            let mux = service.mux.read();
            let Some(tab) = mux.get_tab(&target) else {
                return Ok(None);
            };
            if tab.session_binding_nonce.as_deref() != Some(nonce.as_str())
                || (tab.launch_session_authenticated() && tab.provisional_launch.is_none())
                || tab.pty_id.is_none()
                || !matches!(tab.backing, crate::multiplexer::types::TabBacking::Pty)
            {
                return Ok(None);
            }
            tab.agent_type.clone().map(|provider| {
                (
                    provider,
                    tab.pty_id.clone(),
                    ProviderIdentityEvidence::capture(tab),
                )
            })
        };
        let Some((current_provider, pty_id, evidence)) = identity else {
            return Ok(None);
        };
        let refuse = |reason: &str| {
            log::warn!(
                "[hook-refusal] tab={} event=late_hook reason={}",
                escaped_hook_log_field(&target),
                reason
            );
            (StatusCode::CONFLICT, reason.to_string())
        };
        if provider
            .as_ref()
            .is_some_and(|provider| provider != &current_provider)
        {
            return Err(refuse("provider_mismatch"));
        }
        let Some(session) = session.or_else(|| evidence.session_id.clone()) else {
            return Err(refuse("missing_session"));
        };
        if evidence.session_id.as_deref() != Some(session.as_str())
            && provider_for_claimed_session(
                &service.agent_registry,
                evidence.cwd.as_deref().unwrap_or_default(),
                &session,
            )
            .as_deref()
                != Some(current_provider.as_str())
        {
            return Err(refuse("session_not_owned_by_launch_provider"));
        }
        let claim = service
            .commit_session_binding_custody(&target, || {
                let mut mux = service.mux.write();
                if !mux.get_tab(&target).is_some_and(|tab| {
                    tab.agent_type.as_deref() == Some(current_provider.as_str())
                        && tab.pty_id == pty_id
                }) {
                    return Err((StatusCode::CONFLICT, "launch_identity_changed".into(), None));
                }
                commit_materialized_rebind_session(
                    &mut mux,
                    &target,
                    &current_provider,
                    &session,
                    Some(&nonce),
                    &evidence,
                )
            })
            .map_err(|(_, message, _)| refuse(&message))?;
        Ok(Some((current_provider, session, nonce, claim)))
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    let Some((provider, session, nonce, claim)) = prepared else {
        return Ok(());
    };
    finish_session_binding(
        svc,
        tab_id,
        &provider,
        &session,
        Some(&nonce),
        Some(crate::session::lifecycle::AgentTypeWriteOrigin::Hook),
        "late-verified-hook",
        &serde_json::Value::Null,
        claim,
        crate::session::lifecycle::BindingSource::Hook,
        true,
    )
    .await
    .map(|_| ())
}

#[cfg(test)]
mod tests;
