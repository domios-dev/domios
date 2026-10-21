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
    let header_tab = identity_header(headers, "X-Domios-Tab-Id", "X-CodeTerm-Tab-Id")?; // rename-bridge
    let header_nonce = identity_header(
        headers,
        "X-Domios-Binding-Nonce",
        "X-CodeTerm-Binding-Nonce",
    )?; // rename-bridge
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

fn identity_header<'a>(
    headers: &'a HeaderMap,
    current: &str,
    legacy: &str,
) -> Result<Option<&'a str>, RouteError> {
    headers
        .get(current)
        .or_else(|| headers.get(legacy)) // rename-bridge
        .map(|value| {
            value.to_str().map_err(|_| {
                (
                    StatusCode::FORBIDDEN,
                    "Invalid caller identity header".into(),
                )
            })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn identity_header_reads_new_and_legacy_names() {
        let mut headers = HeaderMap::new();
        headers.insert("X-Domios-Tab-Id", HeaderValue::from_static("new-tab"));
        assert_eq!(
            identity_header(&headers, "X-Domios-Tab-Id", "X-CodeTerm-Tab-Id").unwrap(),
            Some("new-tab")
        );
        headers.clear();
        headers.insert("X-CodeTerm-Tab-Id", HeaderValue::from_static("legacy-tab"));
        assert_eq!(
            identity_header(&headers, "X-Domios-Tab-Id", "X-CodeTerm-Tab-Id").unwrap(),
            Some("legacy-tab")
        );
    }
}
