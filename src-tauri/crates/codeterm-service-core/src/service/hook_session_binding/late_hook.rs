use axum::http::StatusCode;

use super::{bind_reported_session, finish_session_binding};
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
            tab.agent_type
                .clone()
                .zip(session.or_else(|| tab.agent_session_id.clone()))
        };
        let Some((current_provider, session)) = identity else {
            return Ok(None);
        };
        if provider
            .as_ref()
            .is_some_and(|provider| provider != &current_provider)
        {
            return Err((
                StatusCode::CONFLICT,
                "hook provider does not match this launch".to_string(),
            ));
        }
        let claim = service
            .commit_session_binding_custody(&target, || {
                bind_reported_session(
                    &service.mux,
                    &service.agent_registry,
                    &target,
                    &current_provider,
                    &session,
                    Some(&nonce),
                )
            })
            .map_err(|(status, message, _)| (status, message))?;
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
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
