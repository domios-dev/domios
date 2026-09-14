use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use super::hook_routes::{finish_session_binding, SessionBindError, SessionStartClaim};
use super::routes::{resolve_tab, AppState};

#[derive(serde::Deserialize)]
pub(crate) struct RebindSessionRequest {
    session_id: String,
}

#[derive(Debug)]
pub(crate) struct VerifiedRebindCommit {
    pub(crate) provider_id: String,
    pub(crate) previous_session_id: Option<String>,
    pub(crate) binding_nonce: Option<String>,
    pub(crate) claim: SessionStartClaim,
    pub(crate) identity_write: crate::service::provider_identity::AgentTypeWriteOutcome,
}

#[derive(Debug)]
pub(crate) enum VerifiedRebindOutcome {
    Committed(VerifiedRebindCommit),
    EvidenceChanged,
}

pub(crate) async fn binding_state(
    AxumState(state): AxumState<AppState>,
    Path(raw_tab_id): Path<String>,
) -> Result<Json<crate::service::SessionBindingState>, (StatusCode, String)> {
    let tab_id = resolve_tab(&state, &raw_tab_id)
        .map_err(|(status, message)| (status, message.to_string()))?;
    state
        .svc
        .session_binding_state(&tab_id)
        .map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Tab not found".to_string()))
}

pub(crate) async fn rebind_session(
    AxumState(state): AxumState<AppState>,
    Path(raw_tab_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<RebindSessionRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let tab_id = resolve_tab(&state, &raw_tab_id)
        .map_err(|(status, message)| (status, message.to_string()))?;
    let requested = request.session_id.trim();
    if requested.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "session_id is required".to_string(),
        ));
    }
    let caller_tab_id = crate::api::tab_routes::caller_tab_header(&headers);
    let commit = match validate_and_commit_rebind(
        &state.svc,
        &tab_id,
        requested,
        caller_tab_id.as_deref(),
        crate::session::lifecycle::AgentTypeWriteOrigin::Rebind,
    )
    .await
    {
        Ok(VerifiedRebindOutcome::Committed(commit)) => commit,
        Ok(VerifiedRebindOutcome::EvidenceChanged) => {
            return Err((
                StatusCode::CONFLICT,
                "tab session evidence changed before provider identity commit".to_string(),
            ));
        }
        Err((status, message, _)) => {
            let previous = state
                .svc
                .mux
                .read()
                .get_tab(&tab_id)
                .and_then(|tab| tab.agent_session_id.clone());
            let reason = format!(
                "Tab {tab_id} remains bound to {}; requested session '{}' was refused: {message}",
                previous.as_deref().unwrap_or("-"),
                requested,
            );
            state.svc.deferred_session_bindings.refuse(
                &tab_id,
                requested,
                previous.as_deref(),
                &reason,
            );
            state.svc.emit_session_binding_state(&tab_id);
            return Err((status, reason));
        }
    };
    let response = finish_session_binding(
        &state.svc,
        &tab_id,
        &commit.provider_id,
        requested,
        commit.binding_nonce.as_deref(),
        None,
        "cli-rebind",
        &serde_json::Value::Null,
        commit.claim,
        crate::session::lifecycle::BindingSource::Hook,
    )
    .await?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "tab_id": tab_id,
        "found_session_id": commit.previous_session_id,
        "requested_session_id": requested,
        "changed": response.0.get("changed").and_then(|value| value.as_bool()).unwrap_or(false),
    })))
}

#[cfg(test)]
pub(crate) fn rebind_session_core(
    mux: &crate::multiplexer::commands::MuxState,
    registry: &crate::agent::registry::AgentRegistry,
    tab_id: &str,
    provider_id: &str,
    session_id: &str,
    binding_nonce: Option<&str>,
    expected_evidence: &crate::agent::provider_identity::ProviderIdentityEvidence,
) -> Result<SessionStartClaim, SessionBindError> {
    crate::api::hook_routes::bind_validated_rebind_session(
        mux,
        registry,
        tab_id,
        provider_id,
        session_id,
        binding_nonce,
        expected_evidence,
    )
}

pub(crate) async fn validate_and_commit_rebind(
    svc: &crate::service::ServiceContext,
    tab_id: &str,
    session_id: &str,
    caller_tab_id: Option<&str>,
    origin: crate::session::lifecycle::AgentTypeWriteOrigin,
) -> Result<VerifiedRebindOutcome, SessionBindError> {
    if !matches!(
        origin,
        crate::session::lifecycle::AgentTypeWriteOrigin::Rebind
            | crate::session::lifecycle::AgentTypeWriteOrigin::Repair
    ) {
        return Err((
            StatusCode::BAD_REQUEST,
            "verified rebind requires rebind or repair origin".to_string(),
            None,
        ));
    }
    let (previous_provider, previous_session_id, evidence, binding_nonce) = svc
        .mux
        .read()
        .get_tab(tab_id)
        .map(|tab| {
            let evidence = crate::agent::provider_identity::ProviderIdentityEvidence::capture(tab);
            (
                tab.agent_type.clone(),
                tab.agent_session_id.clone(),
                evidence,
                (tab.agent_session_id.as_deref() == Some(session_id)
                    && tab.session_binding_nonce.is_some()
                    && tab.session_binding_nonce == tab.reported_session_binding_nonce)
                    .then(|| tab.session_binding_nonce.clone())
                    .flatten(),
            )
        })
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Tab not found".to_string(), None))?;
    let registry = svc.agent_registry.clone();
    let owned_session_id = session_id.to_string();
    let cwd = evidence.cwd.clone().unwrap_or_default();
    let provider_id = tokio::task::spawn_blocking(move || {
        crate::agent::provider_identity::provider_for_claimed_session(
            &registry,
            &cwd,
            &owned_session_id,
        )
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
    .ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            format!(
                "Session '{}' has no unique provider record; Tab {tab_id} remains provider {}",
                session_id,
                previous_provider.as_deref().unwrap_or("-"),
            ),
            None,
        )
    })?;
    let conditional = svc
        .commit_verified_tab_agent_type_with_origin(
            tab_id,
            provider_id.clone(),
            origin,
            caller_tab_id,
            &evidence,
            |mux| {
                crate::api::hook_routes::commit_materialized_rebind_session(
                    mux,
                    tab_id,
                    &provider_id,
                    session_id,
                    binding_nonce.as_deref(),
                    &evidence,
                )
            },
        )?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Tab not found".to_string(), None))?;
    let (claim, identity_write) = match conditional {
        crate::service::provider_identity::ConditionalAgentTypeWrite::Committed {
            value,
            identity_write,
        } => (value, identity_write),
        crate::service::provider_identity::ConditionalAgentTypeWrite::EvidenceChanged => {
            return Ok(VerifiedRebindOutcome::EvidenceChanged);
        }
    };
    Ok(VerifiedRebindOutcome::Committed(VerifiedRebindCommit {
        provider_id,
        previous_session_id,
        binding_nonce,
        claim,
        identity_write,
    }))
}
