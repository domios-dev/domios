use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

use axum::http::HeaderMap;
use codeterm_types::notification::NotificationAction;
use codeterm_types::pty::{PendingPrompt, PromptIdentity, PromptKind, PromptOption};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::oneshot;

use crate::agent::provider::{HookIngestionRoute, PermissionHookCapability};
use crate::api::routes::{check_auth, AppState};
use crate::notification::dedupe::TabCondition;
use crate::service::{ServiceContext, ServiceState};

const ENABLED_KEY: &str = "permission_hook_decisions";
const DEADLINE: Duration = Duration::from_secs(60);

fn enabled(config: Option<&codeterm_types::config::schema::ProviderConfig>) -> bool {
    config
        .and_then(|config| config.extra.get(ENABLED_KEY))
        .and_then(toml::Value::as_bool)
        == Some(true)
}

fn is_permission_request(body: &Value) -> bool {
    body.get("hook_event_name").and_then(Value::as_str) == Some("PermissionRequest")
}

async fn wait_for_answer(
    receiver: oneshot::Receiver<Answer>,
    deadline: Duration,
) -> Option<Answer> {
    tokio::time::timeout(deadline, receiver)
        .await
        .ok()
        .and_then(Result::ok)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    Allow,
    Deny,
}

struct Request {
    id: String,
    session_id: String,
    nonce: String,
    prompt: PendingPrompt,
    answers: Vec<Answer>,
    sender: Option<oneshot::Sender<Answer>>,
}

#[derive(Default)]
struct Broker {
    requests: HashMap<String, Request>,
}

impl Broker {
    fn register(&mut self, tab_id: &str, request: Request) -> bool {
        if self.requests.contains_key(tab_id) {
            return false;
        }
        self.requests.insert(tab_id.into(), request);
        true
    }

    fn remove(&mut self, tab_id: &str, id: &str) -> Option<Request> {
        if self
            .requests
            .get(tab_id)
            .is_some_and(|request| request.id == id)
        {
            self.requests.remove(tab_id)
        } else {
            None
        }
    }

    fn answer(
        &mut self,
        tab_id: &str,
        session_id: Option<&str>,
        nonce: Option<&str>,
        value: &str,
    ) -> Option<Result<(), String>> {
        let request = self.requests.get_mut(tab_id)?;
        if session_id != Some(request.session_id.as_str()) || nonce != Some(request.nonce.as_str())
        {
            request.sender.take();
            return Some(Err("permission request binding changed".into()));
        }
        let index = request
            .prompt
            .options
            .iter()
            .position(|option| option.value == value)
            .or_else(|| value.parse::<usize>().ok()?.checked_sub(1));
        let Some(answer) = index.and_then(|index| request.answers.get(index)).copied() else {
            return Some(Err("invalid permission option".into()));
        };
        let Some(sender) = request.sender.take() else {
            return Some(Err("permission request already answered".into()));
        };
        Some(
            sender
                .send(answer)
                .map_err(|_| "permission request expired".into()),
        )
    }
}

static BROKER: LazyLock<Mutex<Broker>> = LazyLock::new(|| Mutex::new(Broker::default()));

fn decision(answer: Option<Answer>) -> Value {
    match answer {
        Some(answer) => json!({"hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": {"behavior": match answer { Answer::Allow => "allow", Answer::Deny => "deny" }}
        }}),
        None => json!({}),
    }
}

fn answers(capability: &PermissionHookCapability) -> Option<Vec<Answer>> {
    if capability.protocol != "claude-permission-request"
        || capability.options.len() != 2
        || capability.options.iter().any(|option| {
            option.label.trim().is_empty()
                || option.label.len() > 128
                || option.label.chars().any(char::is_control)
        })
        || capability.options[0].label == capability.options[1].label
    {
        return None;
    }
    let answers: Vec<_> = capability
        .options
        .iter()
        .map(|option| match option.value.as_str() {
            "allow" => Some(Answer::Allow),
            "deny" => Some(Answer::Deny),
            _ => None,
        })
        .collect::<Option<_>>()?;
    (answers.contains(&Answer::Allow) && answers.contains(&Answer::Deny)).then_some(answers)
}

fn prompt(
    tool: &str,
    input: &Value,
    id: &str,
    capability: &PermissionHookCapability,
) -> PendingPrompt {
    let input = serde_json::to_string(input).unwrap_or_default();
    let summary: String = input.chars().take(2000).collect();
    PendingPrompt {
        question: format!("Allow {tool}?\n{summary}"),
        options: capability
            .options
            .iter()
            .enumerate()
            .map(|(index, option)| PromptOption {
                label: option.label.clone(),
                value: format!("permission:{id}:{index}"),
            })
            .collect(),
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    }
}

pub(crate) fn pending_for_tab(tab_id: &str) -> Option<PendingPrompt> {
    BROKER
        .lock()
        .requests
        .get(tab_id)
        .map(|request| request.prompt.clone())
}

pub(crate) fn clear_tab(tab_id: &str) {
    BROKER.lock().requests.remove(tab_id);
}

pub(crate) fn respond(
    svc: &ServiceContext,
    tab_id: &str,
    value: &str,
) -> Option<Result<(), String>> {
    let binding = svc.mux.read().get_tab(tab_id).map(|tab| {
        (
            tab.agent_session_id.clone(),
            tab.session_binding_nonce.clone(),
        )
    });
    let (session, nonce) = binding.unwrap_or_default();
    BROKER
        .lock()
        .answer(tab_id, session.as_deref(), nonce.as_deref(), value)
}

fn notify(
    svc: &ServiceContext,
    tab_id: &str,
    title: &str,
    prompt: &PendingPrompt,
    id: &str,
) -> String {
    let identity = PromptIdentity::new(PromptKind::Structured, prompt);
    let actions = prompt
        .options
        .iter()
        .map(|option| NotificationAction::RespondPromptOption {
            label: option.label.clone(),
            value: option.value.clone(),
            identity: identity.clone(),
        })
        .collect();
    svc.create_condition_notification_with_key(
        tab_id,
        TabCondition::PendingPrompt,
        format!("{title} needs permission"),
        Some(prompt.question.clone()),
        Some(actions),
        format!("permission-hook:{tab_id}:{id}"),
    )
}

struct PendingGuard {
    svc: ServiceState,
    tab_id: String,
    id: String,
    identity: PromptIdentity,
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        BROKER.lock().remove(&self.tab_id, &self.id);
        let current = self
            .svc
            .mux
            .read()
            .get_tab(&self.tab_id)
            .and_then(|tab| tab.prompt_identity());
        if current.as_ref() == Some(&self.identity) {
            self.svc.push_structured_prompt(&self.tab_id, None);
        }
        self.svc
            .resolve_prompt_notifications(&self.tab_id, &self.identity);
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
        || tab.pending_prompt.is_some()
        || crate::service::structured_prompt::pending_for_tab(tab_id).is_some()
    {
        return no_decision();
    }
    let signal = crate::agent::permission_request::parse_permission_request(body)?;
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
    state
        .svc
        .push_structured_prompt(tab_id, Some(prompt.clone()));
    notify(&state.svc, tab_id, &tab.title, &prompt, &id);
    let answer = wait_for_answer(receiver, DEADLINE).await;
    let binding_matches = state.svc.mux.read().get_tab(tab_id).is_some_and(|current| {
        current.agent_session_id == tab.agent_session_id
            && current.session_binding_nonce == tab.session_binding_nonce
    });
    Some(decision(answer.filter(|_| binding_matches)))
}

#[cfg(test)]
mod tests;
