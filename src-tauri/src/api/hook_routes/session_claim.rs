use axum::http::StatusCode;

use super::{DeferredBindingPremise, SessionBindError, SessionBindingSource, SessionStartClaim};

struct MaterializationProbe {
    cwd: String,
    previous_session_id: Option<String>,
    requested_materialized: bool,
    previous_materialized: bool,
}

pub(crate) fn bind_reported_session(
    mux: &crate::multiplexer::commands::MuxState,
    registry: &crate::agent::registry::AgentRegistry,
    tab_id: &str,
    provider_id: &str,
    session_id: &str,
    report_nonce: Option<&str>,
) -> Result<SessionStartClaim, SessionBindError> {
    bind_session(
        mux,
        registry,
        tab_id,
        provider_id,
        session_id,
        report_nonce,
        SessionBindingSource::Hook,
        None,
    )
}

#[cfg(test)]
pub(crate) fn bind_validated_rebind_session(
    mux: &crate::multiplexer::commands::MuxState,
    registry: &crate::agent::registry::AgentRegistry,
    tab_id: &str,
    provider_id: &str,
    session_id: &str,
    binding_nonce: Option<&str>,
    expected_evidence: &crate::agent::provider_identity::ProviderIdentityEvidence,
) -> Result<SessionStartClaim, SessionBindError> {
    bind_session(
        mux,
        registry,
        tab_id,
        provider_id,
        session_id,
        binding_nonce,
        SessionBindingSource::ValidatedRebind,
        Some(expected_evidence),
    )
}

pub(crate) fn commit_materialized_rebind_session(
    mux: &mut crate::multiplexer::state::MultiplexerState,
    tab_id: &str,
    provider_id: &str,
    session_id: &str,
    binding_nonce: Option<&str>,
    expected_evidence: &crate::agent::provider_identity::ProviderIdentityEvidence,
) -> Result<SessionStartClaim, SessionBindError> {
    let probe = MaterializationProbe {
        cwd: expected_evidence.cwd.clone().unwrap_or_default(),
        previous_session_id: expected_evidence.session_id.clone(),
        requested_materialized: true,
        previous_materialized: false,
    };
    commit_reported_session(
        mux,
        tab_id,
        provider_id,
        session_id,
        binding_nonce,
        &probe,
        SessionBindingSource::ValidatedRebind,
        Some(expected_evidence),
    )?
    .ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            "tab session evidence changed during provider validation".to_string(),
            None,
        )
    })
}

fn bind_session(
    mux: &crate::multiplexer::commands::MuxState,
    registry: &crate::agent::registry::AgentRegistry,
    tab_id: &str,
    provider_id: &str,
    session_id: &str,
    report_nonce: Option<&str>,
    source: SessionBindingSource,
    expected_evidence: Option<&crate::agent::provider_identity::ProviderIdentityEvidence>,
) -> Result<SessionStartClaim, SessionBindError> {
    if registry.with_provider(provider_id, |_| ()).is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("unknown provider_id '{}'", provider_id),
            None,
        ));
    }

    // Provider stores may do IO, so probe without the mux lock and commit only
    // while the tab still matches the probed identity.
    const MAX_ATTEMPTS: usize = 4;
    for _ in 0..MAX_ATTEMPTS {
        let (cwd, previous_session_id) = {
            let mux = mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return Err((StatusCode::NOT_FOUND, "Tab not found".to_string(), None));
            };
            (
                tab.cwd.clone().unwrap_or_default(),
                tab.agent_session_id.clone(),
            )
        };

        let requested_materialized = registry
            .with_provider(provider_id, |provider| {
                provider.session_exists(&cwd, session_id)
            })
            .unwrap_or(false);
        let previous_materialized = if !requested_materialized {
            match previous_session_id.as_deref() {
                Some(previous) if previous != session_id => registry
                    .with_provider(provider_id, |provider| {
                        provider.session_exists(&cwd, previous)
                    })
                    .unwrap_or(false),
                _ => false,
            }
        } else {
            false
        };
        let probe = MaterializationProbe {
            cwd,
            previous_session_id,
            requested_materialized,
            previous_materialized,
        };

        let mut mux = mux.write();
        match commit_reported_session(
            &mut mux,
            tab_id,
            provider_id,
            session_id,
            report_nonce,
            &probe,
            source,
            expected_evidence,
        )? {
            Some(claim) => return Ok(claim),
            None => continue,
        }
    }

    Err((
        StatusCode::CONFLICT,
        "session binding raced concurrent mutation; retry".to_string(),
        None,
    ))
}

fn commit_reported_session(
    mux: &mut crate::multiplexer::state::MultiplexerState,
    tab_id: &str,
    provider_id: &str,
    session_id: &str,
    report_nonce: Option<&str>,
    probe: &MaterializationProbe,
    source: SessionBindingSource,
    expected_evidence: Option<&crate::agent::provider_identity::ProviderIdentityEvidence>,
) -> Result<Option<SessionStartClaim>, SessionBindError> {
    let Some(tab) = mux.get_tab(tab_id) else {
        return Err((StatusCode::NOT_FOUND, "Tab not found".to_string(), None));
    };
    if tab.cwd.clone().unwrap_or_default() != probe.cwd
        || tab.agent_session_id != probe.previous_session_id
    {
        return Ok(None);
    }
    if source == SessionBindingSource::ValidatedRebind
        && expected_evidence.is_some_and(|expected| {
            crate::agent::provider_identity::ProviderIdentityEvidence::capture(tab) != *expected
        })
    {
        return Err((
            StatusCode::CONFLICT,
            "tab session evidence changed during provider validation".to_string(),
            None,
        ));
    }

    let launch_nonce = tab.session_binding_nonce.clone();
    let acknowledged_nonce = tab.reported_session_binding_nonce.clone();
    let nonce_verified = if let Some(expected) = tab.session_binding_nonce.clone() {
        match report_nonce {
            Some(reported) if reported != expected => {
                log::warn!(
                    "[hook_session_start] rejecting sid {} for Tab {} provider={}: binding nonce mismatch",
                    session_id, tab_id, provider_id,
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

    if source == SessionBindingSource::Hook {
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
    }

    if !nonce_verified && !probe.requested_materialized {
        log::warn!(
            "[hook_session_start] rejecting sid {} for Tab {} provider={}: session does not belong to provider",
            session_id,
            tab_id,
            provider_id,
        );
        return Err((
            StatusCode::CONFLICT,
            format!(
                "session_id '{}' is not a known session for provider '{}'",
                session_id, provider_id
            ),
            None,
        ));
    }

    if let Some(other) = mux
        .tab_ids_with_session(session_id)
        .into_iter()
        .find(|other| other != tab_id)
    {
        log::warn!(
            "[hook_session_start] rejecting sid {} for Tab {} provider={} already claimed by {}",
            session_id,
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

    if probe.previous_materialized {
        let nonce_already_acknowledged =
            launch_nonce.is_some() && acknowledged_nonce == launch_nonce;
        let fresh_authenticated_launch = nonce_verified && !nonce_already_acknowledged;
        if !fresh_authenticated_launch {
            log::warn!(
                "[hook_session_start] rejecting fileless sid {} for Tab {} provider={}: would evict materialized session {:?}",
                session_id, tab_id, provider_id, probe.previous_session_id,
            );
            return Err((
                StatusCode::CONFLICT,
                format!(
                    "session_id '{}' cannot evict materialized session {:?}",
                    session_id, probe.previous_session_id
                ),
                probe
                    .previous_session_id
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
    let previous = tab.agent_session_id.clone();
    let changed = previous.as_deref() != Some(session_id);
    if changed {
        if let Some(previous_session_id) = previous.as_deref() {
            tab.archive_context_epoch(provider_id, previous_session_id);
        }
        tab.agent_session_id = Some(session_id.to_string());
        crate::service::session_model::clear_runtime_model_evidence(tab);
    }
    tab.reported_session_binding_nonce = if nonce_verified {
        Some(report_nonce.unwrap_or_default().to_string())
    } else if !changed {
        acknowledged_nonce
    } else {
        None
    };
    Ok(Some(SessionStartClaim {
        prev: previous,
        changed,
        nonce_verified,
        identity_evidence: crate::agent::provider_identity::ProviderIdentityEvidence::capture(tab),
    }))
}
