use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};

use super::routes::AppState;
use codeterm_service_core::service::hook_session_binding::{
    finish_session_binding, SessionBindError, SessionStartClaim,
};

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
    pub(crate) identity_write:
        codeterm_service_core::service::provider_identity::AgentTypeWriteOutcome,
}

#[derive(Debug)]
pub(crate) enum VerifiedRebindOutcome {
    Committed(VerifiedRebindCommit),
    EvidenceChanged,
}

pub(crate) async fn binding_state(
    AxumState(state): AxumState<AppState>,
    Path(raw_tab_id): Path<String>,
) -> Result<Json<codeterm_service_core::service::SessionBindingState>, (StatusCode, String)> {
    let tab_id = crate::routes::resolve_tab_async(&state, &raw_tab_id)
        .await
        .map_err(|(status, message)| (status, message.to_string()))?;
    tokio::task::spawn_blocking(move || {
        state
            .svc
            .session_binding_state(&tab_id)
            .map(Json)
            .ok_or_else(|| (StatusCode::NOT_FOUND, "Tab not found".to_string()))
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed".to_string(),
        )
    })?
}

pub(crate) async fn rebind_session(
    AxumState(state): AxumState<AppState>,
    Path(raw_tab_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<RebindSessionRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let tab_id = crate::routes::resolve_tab_async(&state, &raw_tab_id)
        .await
        .map_err(|(status, message)| (status, message.to_string()))?;
    let requested = request.session_id.trim();
    if requested.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "session_id is required".to_string(),
        ));
    }
    let caller_tab_id = crate::tab_routes::caller_tab_header(&headers);
    let commit = match validate_and_commit_rebind(
        &state.svc,
        &tab_id,
        requested,
        caller_tab_id.as_deref(),
        codeterm_service_core::session::lifecycle::AgentTypeWriteOrigin::Rebind,
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
            let owner = state.clone();
            let target = tab_id.clone();
            let previous = tokio::task::spawn_blocking(move || {
                let state = owner;
                let tab_id = target;
                let mux = state.svc.mux.read();
                mux.get_tab(&tab_id)
                    .and_then(|tab| tab.agent_session_id.clone())
            })
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "blocking task failed".to_string(),
                )
            })?;
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
            let owner = state.svc.clone();
            let target = tab_id.clone();
            tokio::task::spawn_blocking(move || owner.emit_session_binding_state(&target))
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "blocking task failed".to_string(),
                    )
                })?;
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
        codeterm_service_core::session::lifecycle::BindingSource::Hook,
        false,
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
    mux: &codeterm_service_core::multiplexer::commands::MuxState,
    registry: &codeterm_service_core::agent::registry::AgentRegistry,
    tab_id: &str,
    provider_id: &str,
    session_id: &str,
    binding_nonce: Option<&str>,
    expected_evidence: &codeterm_service_core::agent::provider_identity::ProviderIdentityEvidence,
) -> Result<SessionStartClaim, SessionBindError> {
    codeterm_service_core::service::hook_session_binding::bind_validated_rebind_session(
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
    svc: &codeterm_service_core::service::ServiceContext,
    tab_id: &str,
    session_id: &str,
    caller_tab_id: Option<&str>,
    origin: codeterm_service_core::session::lifecycle::AgentTypeWriteOrigin,
) -> Result<VerifiedRebindOutcome, SessionBindError> {
    if !matches!(
        origin,
        codeterm_service_core::session::lifecycle::AgentTypeWriteOrigin::Rebind
            | codeterm_service_core::session::lifecycle::AgentTypeWriteOrigin::Repair
    ) {
        return Err((
            StatusCode::BAD_REQUEST,
            "verified rebind requires rebind or repair origin".to_string(),
            None,
        ));
    }
    let owner = svc.clone();
    let target = tab_id.to_owned();
    let session = session_id.to_owned();
    let (previous_provider, previous_session_id, evidence, binding_nonce, live_owner) =
        tokio::task::spawn_blocking(move || {
            let svc = owner;
            let tab_id = target.as_str();
            let session_id = session.as_str();
            let mux = svc.mux.read();
            mux.get_tab(tab_id)
                .map(|tab| {
                    let evidence =
                codeterm_service_core::agent::provider_identity::ProviderIdentityEvidence::capture(
                    tab,
                );
                    (
                        tab.agent_type.clone(),
                        tab.agent_session_id.clone(),
                        evidence,
                        (tab.agent_session_id.as_deref() == Some(session_id)
                            && tab.session_binding_nonce.is_some()
                            && tab.session_binding_nonce == tab.reported_session_binding_nonce)
                            .then(|| tab.session_binding_nonce.clone())
                            .flatten(),
                        codeterm_service_core::service::tab_has_live_agent(tab),
                    )
                })
                .ok_or_else(|| (StatusCode::NOT_FOUND, "Tab not found".to_string(), None))
        })
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "blocking task failed".to_string(),
                None,
            )
        })??;
    let registry = svc.agent_registry.clone();
    let owned_session_id = session_id.to_string();
    let cwd = evidence.cwd.clone().unwrap_or_default();
    let provider_id = tokio::task::spawn_blocking(move || {
        codeterm_service_core::agent::provider_identity::provider_for_claimed_session(
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
    let owner = svc.clone();
    let target = tab_id.to_owned();
    let session = session_id.to_owned();
    let caller = caller_tab_id.map(str::to_owned);
    tokio::task::spawn_blocking(move || {
let svc = owner;
        let tab_id = target.as_str();
        let session_id = session.as_str();
        let caller_tab_id = caller.as_deref();
    if let Some(owner) = crate::hook_routes::foreign_provider_session_owner(
        previous_provider.as_deref(),
        &provider_id,
        live_owner,
    ) {
        return Err((
            StatusCode::CONFLICT,
            format!(
                "Session '{session_id}' belongs to provider {provider_id}, but Tab {tab_id} runs a verified {owner} agent; a rebind never switches the provider of a live agent"
            ),
            None,
        ));
    }
    let conditional = svc
        .commit_verified_tab_agent_type_with_origin(
            tab_id,
            provider_id.clone(),
            origin,
            caller_tab_id,
            &evidence,
            |mux| {
                codeterm_service_core::service::hook_session_binding::commit_materialized_rebind_session(
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
        codeterm_service_core::service::provider_identity::ConditionalAgentTypeWrite::Committed {
            value,
            identity_write,
        } => (value, identity_write),
        codeterm_service_core::service::provider_identity::ConditionalAgentTypeWrite::EvidenceChanged => {
            return Ok(VerifiedRebindOutcome::EvidenceChanged);
        }
    };
    svc.reconcile_validated_rebind_boot(tab_id, session_id);
    Ok(VerifiedRebindOutcome::Committed(VerifiedRebindCommit {
        provider_id,
        previous_session_id,
        binding_nonce,
        claim,
        identity_write,
    }))

}).await.map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "blocking task failed".to_string(), None))?
}
