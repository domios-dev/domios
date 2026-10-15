use axum::http::HeaderMap;
use codeterm_types::pty::{PromptIdentity, PromptKind};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::routes::{check_auth, AppState};
use codeterm_service_core::agent::provider::HookIngestionRoute;
use codeterm_service_core::service::permission_decision::{
    answers, decision, enabled, is_permission_request, notify, prompt, sync_display,
    wait_for_answer, PendingGuard, Request, BROKER, DEADLINE,
};

pub(crate) async fn handle(
    state: &AppState,
    tab_id: &str,
    headers: &HeaderMap,
    body: &Value,
) -> Option<Value> {
    if !is_permission_request(body) {
        return None;
    }
    let tab = state.svc.mux.read().get_tab(tab_id).cloned()?;
    let provider_id = tab.agent_type.as_deref()?;
    let enabled = enabled(state.config.lock().agents.get(provider_id));
    if !enabled {
        return None;
    }
    let capability = state
        .svc
        .agent_registry
        .with_provider(provider_id, |provider| {
            provider
                .hook_event_roster()
                .iter()
                .any(|binding| {
                    binding.event == "PermissionRequest"
                        && binding.route == HookIngestionRoute::Permission
                })
                .then(|| provider.permission_hook_capability())
                .flatten()
        })
        .flatten()?;
    let answers = answers(&capability)?;
    let no_decision = || Some(decision(None));
    let session = body.get("session_id").and_then(Value::as_str);
    let nonce = super::header_str(headers, "x-codeterm-binding-nonce");
    if check_auth(state, headers).is_err()
        || !session.is_some_and(|session| !session.trim().is_empty())
        || session != tab.agent_session_id.as_deref()
        || !nonce.is_some_and(|nonce| !nonce.trim().is_empty())
        || nonce != tab.session_binding_nonce.as_deref()
    {
        return no_decision();
    }
    let signal = codeterm_service_core::agent::permission_request::parse_permission_request(body)?;
    if signal.tool_name.len() > 256 {
        return no_decision();
    }
    let id = uuid::Uuid::new_v4().to_string();
    let prompt = prompt(&signal.tool_name, &signal.tool_input, &id, &capability);
    let identity = PromptIdentity::new(PromptKind::Structured, &prompt);
    let (sender, receiver) = oneshot::channel();
    let request = Request {
        id: id.clone(),
        session_id: session?.into(),
        nonce: nonce?.into(),
        prompt: prompt.clone(),
        answers,
        sender: Some(sender),
    };
    if !BROKER.lock().register(tab_id, request) {
        return no_decision();
    }
    let _guard = PendingGuard {
        svc: state.svc.clone(),
        tab_id: tab_id.into(),
        id: id.clone(),
        identity: identity.clone(),
    };
    sync_display(&state.svc, tab_id);
    notify(&state.svc, tab_id, &tab.title, &prompt, &id);
    let answer = wait_for_answer(receiver, DEADLINE).await;
    let binding_matches = state.svc.mux.read().get_tab(tab_id).is_some_and(|current| {
        current.agent_session_id == tab.agent_session_id
            && current.session_binding_nonce == tab.session_binding_nonce
    });
    Some(decision(answer.filter(|_| binding_matches)))
}
