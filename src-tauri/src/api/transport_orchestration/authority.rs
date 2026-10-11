use super::{AppState, RouteError};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;

#[derive(Default, Deserialize)]
pub struct CallerClaim {
    pub caller_tab_id: Option<String>,
    pub session_binding_nonce: Option<String>,
}

pub fn require_owner(local: bool) -> Result<(), RouteError> {
    if local {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            "Authenticated owner identity required".into(),
        ))
    }
}

pub fn validate_nonce(
    claimed: Option<&str>,
    supplied: Option<&str>,
    current: Option<&str>,
    exists: bool,
) -> Result<(), RouteError> {
    match claimed {
        None if supplied.is_none() => Ok(()),
        Some(tab)
            if !tab.trim().is_empty()
                && exists
                && supplied.is_some_and(|nonce| !nonce.is_empty() && Some(nonce) == current) =>
        {
            Ok(())
        }
        _ => Err((
            StatusCode::FORBIDDEN,
            "Caller tab requires its current session binding nonce".into(),
        )),
    }
}

pub fn validate_claim(
    state: &AppState,
    headers: &HeaderMap,
    claim: &CallerClaim,
) -> Result<Option<String>, RouteError> {
    require_owner(super::is_local_caller(state, headers))?;
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let header_tab = header("X-CodeTerm-Tab-Id");
    let header_nonce = header("X-CodeTerm-Binding-Nonce");
    if header_tab
        .zip(claim.caller_tab_id.as_deref())
        .is_some_and(|(a, b)| a != b)
        || header_nonce
            .zip(claim.session_binding_nonce.as_deref())
            .is_some_and(|(a, b)| a != b)
    {
        return Err((
            StatusCode::FORBIDDEN,
            "Conflicting caller identity claims".into(),
        ));
    }
    let claimed = claim.caller_tab_id.as_deref().or(header_tab);
    let supplied = claim.session_binding_nonce.as_deref().or(header_nonce);
    let mux = state.svc.mux.read();
    let tab = claimed.and_then(|id| mux.get_tab(id));
    validate_nonce(
        claimed,
        supplied,
        tab.and_then(|tab| tab.session_binding_nonce.as_deref()),
        tab.is_some(),
    )?;
    Ok(claimed.map(str::to_owned))
}
