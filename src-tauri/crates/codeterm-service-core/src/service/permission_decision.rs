use std::collections::{HashMap, VecDeque};
use std::sync::LazyLock;
use std::time::Duration;

use codeterm_types::pty::{PendingPrompt, PromptIdentity, PromptKind, PromptOption};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::oneshot;

use crate::agent::provider::PermissionHookCapability;
use crate::service::{ServiceContext, ServiceState};

const ENABLED_KEY: &str = "permission_hook_decisions";
pub const DEADLINE: Duration = Duration::from_secs(60);

pub fn enabled(config: Option<&codeterm_types::config::schema::ProviderConfig>) -> bool {
    config
        .and_then(|config| config.extra.get(ENABLED_KEY))
        .and_then(toml::Value::as_bool)
        == Some(true)
}

pub fn is_permission_request(body: &Value) -> bool {
    body.get("hook_event_name").and_then(Value::as_str) == Some("PermissionRequest")
}

pub async fn wait_for_answer(
    receiver: oneshot::Receiver<Answer>,
    deadline: Duration,
) -> Option<Answer> {
    tokio::time::timeout(deadline, receiver)
        .await
        .ok()
        .and_then(Result::ok)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Allow,
    Deny,
}

pub struct Request {
    pub id: String,
    pub session_id: String,
    pub nonce: String,
    pub prompt: PendingPrompt,
    pub answers: Vec<Answer>,
    pub sender: Option<oneshot::Sender<Answer>>,
}

#[derive(Default)]
pub struct Broker {
    requests: HashMap<String, VecDeque<Request>>,
}

impl Broker {
    pub fn register(&mut self, tab_id: &str, request: Request) -> bool {
        let requests = self.requests.entry(tab_id.into()).or_default();
        if requests.iter().any(|pending| pending.id == request.id) {
            return false;
        }
        requests.push_back(request);
        true
    }

    fn remove(&mut self, tab_id: &str, id: &str) -> Option<Request> {
        let requests = self.requests.get_mut(tab_id)?;
        let index = requests.iter().position(|request| request.id == id)?;
        let removed = requests.remove(index);
        if requests.is_empty() {
            self.requests.remove(tab_id);
        }
        removed
    }

    fn answer(
        &mut self,
        tab_id: &str,
        session_id: Option<&str>,
        nonce: Option<&str>,
        value: &str,
    ) -> Option<Result<(), String>> {
        let requests = self.requests.get_mut(tab_id)?;
        let index = requests.iter().position(|request| {
            request
                .prompt
                .options
                .iter()
                .any(|option| option.value == value)
        });
        let request = match index {
            Some(index) => requests.get_mut(index)?,
            None if value.starts_with("permission:") => {
                return Some(Err("permission request expired".into()))
            }
            None => requests.front_mut()?,
        };
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

pub static BROKER: LazyLock<Mutex<Broker>> = LazyLock::new(|| Mutex::new(Broker::default()));

pub fn decision(answer: Option<Answer>) -> Value {
    match answer {
        Some(answer) => json!({"hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": {"behavior": match answer { Answer::Allow => "allow", Answer::Deny => "deny" }}
        }}),
        None => json!({}),
    }
}

pub fn answers(capability: &PermissionHookCapability) -> Option<Vec<Answer>> {
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

pub fn prompt(
    tool: &str,
    input: &Value,
    id: &str,
    capability: &PermissionHookCapability,
) -> PendingPrompt {
    let input = serde_json::to_string(input).unwrap_or_default();
    let summary: String = input.chars().take(2000).collect();
    PendingPrompt {
        request_id: None,
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
        .and_then(|requests| requests.front())
        .map(|request| request.prompt.clone())
}

pub fn clear_tab(tab_id: &str) {
    BROKER.lock().requests.remove(tab_id);
}

pub fn notify(
    svc: &ServiceContext,
    tab_id: &str,
    title: &str,
    prompt: &PendingPrompt,
    id: &str,
) -> String {
    svc.log_system_notice(
        format!("{title} needs permission"),
        Some(format!("Tab {tab_id}, request {id}: {}", prompt.question)),
        Some("warning"),
    )
}

pub struct PendingGuard {
    pub svc: ServiceState,
    pub tab_id: String,
    pub id: String,
    pub identity: PromptIdentity,
}

fn hook_identity(identity: &PromptIdentity) -> bool {
    identity.kind == PromptKind::Structured
        && identity
            .options
            .iter()
            .any(|option| option.value.starts_with("permission:"))
}

pub fn sync_display(svc: &ServiceContext, tab_id: &str) {
    let prompt = {
        let mut mux = svc.mux.write();
        let Some(tab) = mux.get_tab_mut(tab_id) else {
            return;
        };
        if tab
            .prompt_identity()
            .is_some_and(|identity| !hook_identity(&identity))
        {
            return;
        }
        let prompt = pending_for_tab(tab_id);
        if tab.pending_prompt == prompt {
            return;
        }
        tab.pending_prompt = prompt.clone();
        tab.pending_prompt_kind = prompt.as_ref().map(|_| PromptKind::Structured);
        prompt
    };
    crate::events::emit_everywhere(
        &svc.app_handle,
        "prompt-state-changed",
        json!({"tab_id":tab_id,"prompt":prompt}),
    );
    svc.emit_state_changed();
}

pub(crate) fn respond_with_identity(
    svc: &ServiceContext,
    tab_id: &str,
    value: &str,
    expected: Option<&PromptIdentity>,
) -> Option<Result<(), String>> {
    let displayed_identity = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.prompt_identity());
    let expected = response_identity(value, expected, displayed_identity.as_ref())?;
    let binding = svc.mux.read().get_tab(tab_id).map(|tab| {
        (
            tab.agent_session_id.clone(),
            tab.session_binding_nonce.clone(),
        )
    });
    let (session, nonce) = binding.unwrap_or_default();
    let (result, identity, index) = {
        let mut broker = BROKER.lock();
        let requests = broker.requests.get(tab_id)?;
        let request = if let Some(expected) = expected {
            if !hook_identity(expected) {
                return None;
            }
            requests.iter().find(|request| {
                PromptIdentity::new(PromptKind::Structured, &request.prompt) == *expected
            })
        } else {
            requests
                .iter()
                .find(|request| {
                    request
                        .prompt
                        .options
                        .iter()
                        .any(|option| option.value == value)
                })
                .or_else(|| value.parse::<usize>().ok().and_then(|_| requests.front()))
        };
        let Some(request) = request else {
            return Some(Err("permission request expired".into()));
        };
        let identity = PromptIdentity::new(PromptKind::Structured, &request.prompt);
        let index = request
            .prompt
            .options
            .iter()
            .position(|option| option.value == value)
            .or_else(|| value.parse::<usize>().ok()?.checked_sub(1));
        let Some(option) = index.and_then(|index| request.prompt.options.get(index)) else {
            return Some(Err("invalid permission option".into()));
        };
        let routed_value = option.value.clone();
        let result = broker.answer(tab_id, session.as_deref(), nonce.as_deref(), &routed_value)?;
        (result, identity, index)
    };
    if result.is_ok() {
        if let Some(index) = index {
            svc.answer_prompt_notifications(tab_id, &identity, index, "tab_respond");
        }
    }
    Some(result)
}

fn response_identity<'a>(
    value: &str,
    expected: Option<&'a PromptIdentity>,
    displayed: Option<&'a PromptIdentity>,
) -> Option<Option<&'a PromptIdentity>> {
    if let Some(expected) = expected {
        return hook_identity(expected).then_some(Some(expected));
    }
    if value.starts_with("permission:") {
        return Some(None);
    }
    if value.parse::<usize>().is_ok() {
        return displayed
            .filter(|identity| hook_identity(identity))
            .map(Some);
    }
    None
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        BROKER.lock().remove(&self.tab_id, &self.id);
        sync_display(&self.svc, &self.tab_id);
        let resolved = self
            .svc
            .notif
            .lock()
            .resolve_prompt_notifications(&self.tab_id, &self.identity);
        self.svc.announce_resolved(&resolved);
    }
}

#[cfg(test)]
mod tests;
