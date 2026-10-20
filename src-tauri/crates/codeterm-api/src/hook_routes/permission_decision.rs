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

struct OffRuntimePendingGuard(Option<PendingGuard>);

impl OffRuntimePendingGuard {
    fn take(mut self) -> Option<PendingGuard> {
        self.0.take()
    }
}

impl Drop for OffRuntimePendingGuard {
    fn drop(&mut self) {
        if let Some(guard) = self.0.take() {
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn_blocking(move || drop(guard));
            } else {
                drop(guard);
            }
        }
    }
}

pub(crate) async fn handle(
    state: &AppState,
    tab_id: &str,
    headers: &HeaderMap,
    body: &Value,
) -> Option<Value> {
    if !is_permission_request(body) {
        return None;
    }
    let owner = state.svc.clone();
    let target = tab_id.to_owned();
    let tab = tokio::task::spawn_blocking(move || owner.mux.read().get_tab(&target).cloned())
        .await
        .ok()??;
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
    let guard = OffRuntimePendingGuard(Some(PendingGuard {
        svc: state.svc.clone(),
        tab_id: tab_id.into(),
        id: id.clone(),
        identity: identity.clone(),
    }));
    let owner = state.svc.clone();
    let target = tab_id.to_owned();
    let title = tab.title.clone();
    let display_prompt = prompt.clone();
    let display_id = id.clone();
    tokio::task::spawn_blocking(move || {
        sync_display(&owner, &target);
        notify(&owner, &target, &title, &display_prompt, &display_id);
    })
    .await
    .ok()?;
    let answer = wait_for_answer(receiver, DEADLINE).await;
    let owner = state.svc.clone();
    let target = tab_id.to_owned();
    tokio::task::spawn_blocking(move || {
        let _guard = guard.take();
        let state = owner;
        let tab_id = target.as_str();
        let binding_matches = state.mux.read().get_tab(tab_id).is_some_and(|current| {
            current.agent_session_id == tab.agent_session_id
                && current.session_binding_nonce == tab.session_binding_nonce
        });
        Some(decision(answer.filter(|_| binding_matches)))
    })
    .await
    .ok()?
}
