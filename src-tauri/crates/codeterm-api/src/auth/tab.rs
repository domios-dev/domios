use axum::{
    extract::Request,
    http::{HeaderMap, StatusCode},
};
use codeterm_service_core::service::tab_credentials::TabCaller;

use super::{identify_caller, CallerIdentity};
use crate::routes::AppState;

pub(crate) fn tab_caller(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<TabCaller>, (StatusCode, String)> {
    let auth = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    match identify_caller(state, auth) {
        Ok(CallerIdentity::Tab(caller)) => Ok(Some(caller)),
        Ok(CallerIdentity::Local) => Ok(None),
        _ => Err((
            StatusCode::FORBIDDEN,
            "Local caller credential required".into(),
        )),
    }
}

pub(crate) fn caller_claim(
    state: &AppState,
    headers: &HeaderMap,
    claim: Option<&str>,
) -> Result<Option<String>, (StatusCode, String)> {
    match tab_caller(state, headers)? {
        Some(caller) => {
            caller
                .validate_claim(claim)
                .map_err(|error| (StatusCode::FORBIDDEN, error))?;
            Ok(Some(caller.tab_id))
        }
        None if claim.is_none() => Ok(None),
        None => {
            let claim = claim
                .filter(|claim| !claim.is_empty())
                .ok_or((StatusCode::FORBIDDEN, "Empty selected tab context".into()))?;
            Ok(Some(claim.to_owned()))
        }
    }
}

pub(crate) fn actor_ref(
    state: &AppState,
    headers: &HeaderMap,
    selected: Option<codeterm_service_core::plan::AgentRef>,
) -> Result<codeterm_service_core::plan::AgentRef, (StatusCode, String)> {
    let Some(caller) = tab_caller(state, headers)? else {
        return Ok(
            selected.unwrap_or_else(|| codeterm_service_core::plan::AgentRef::from_tab("unknown"))
        );
    };
    caller
        .validate_claim(
            selected
                .as_ref()
                .and_then(|selected| selected.tab_id.as_deref()),
        )
        .map_err(|error| (StatusCode::FORBIDDEN, error))?;
    let mux = state.svc.mux.read();
    let tab = mux
        .get_tab(&caller.tab_id)
        .ok_or((StatusCode::FORBIDDEN, "Caller tab has closed".into()))?;
    Ok(codeterm_service_core::plan::AgentRef {
        tab_id: Some(caller.tab_id),
        session_id: tab.agent_session_id.clone(),
        title: Some(tab.title.clone()),
    })
}

pub(super) fn bind_headers(
    state: &AppState,
    req: &mut Request,
    caller: &CallerIdentity,
) -> Result<(), &'static str> {
    if matches!(caller, CallerIdentity::Local) {
        return Ok(());
    }
    let expected = match caller {
        CallerIdentity::Tab(caller) => Some(caller),
        _ => None,
    };
    if let Some(claim) = req.headers().get("x-codeterm-tab-id") {
        if claim.to_str().ok() != expected.map(|caller| caller.tab_id.as_str()) {
            return Err("Caller header does not match its credential");
        }
    }
    for name in [
        "x-codeterm-binding-nonce",
        "x-codeterm-session-binding-nonce",
    ] {
        if let Some(claim) = req.headers().get(name) {
            if claim.to_str().ok() != expected.map(|caller| caller.binding_nonce.as_str()) {
                return Err("Caller nonce header does not match its credential");
            }
        }
    }
    if let Some(caller) = expected {
        if caller.binding_nonce.is_empty() {
            return Err("Caller launch has ended");
        }
        let mux = state.svc.mux.read();
        let tab = mux.get_tab(&caller.tab_id).ok_or("Caller tab has closed")?;
        if tab.session_binding_nonce.as_deref() != Some(caller.binding_nonce.as_str()) {
            return Err("Caller launch has changed");
        }
        if let Some(claim) = req.headers().get("x-codeterm-session-id") {
            if claim.to_str().ok() != tab.agent_session_id.as_deref() {
                return Err("Caller session header does not match its credential");
            }
        }
        let headers = req.headers_mut();
        for (name, value) in [
            ("x-codeterm-tab-id", Some(caller.tab_id.as_str())),
            (
                "x-codeterm-binding-nonce",
                Some(caller.binding_nonce.as_str()),
            ),
            ("x-codeterm-session-id", tab.agent_session_id.as_deref()),
            ("x-codeterm-worker-id", tab.worker_id.as_deref()),
        ] {
            headers.remove(name);
            if let Some(value) = value {
                headers.insert(
                    name,
                    value.parse().map_err(|_| "Invalid live caller identity")?,
                );
            }
        }
        if let Some((target, _)) = super::tab_hook_target(req.method(), req.uri().path()) {
            if target != caller.tab_id {
                return Err("Hook target does not match its credential");
            }
        }
    } else {
        for name in ["x-codeterm-session-id", "x-codeterm-worker-id"] {
            if req.headers().contains_key(name) {
                return Err("Tab identity headers require a tab credential");
            }
        }
    }
    Ok(())
}
