mod live_model_switch;
pub mod opencode_sse;
mod questions;
mod reconcile;
mod recovery;
mod session_events;
pub(crate) mod snapshot;
mod subscriber;
mod subscription;
#[cfg(test)]
mod tests;
mod tool_diffs;

pub(crate) use live_model_switch::{live_switch_for_tab, same_model_id, LiveModelSwitchResult};

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use crate::lock_hold::TimedMutex;
use parking_lot::Mutex;

use opencode_sse::{
    normalize_chat_part, parse_event, reply_endpoint, ChatPartEvent, ChatPartUpdate, MessageMeta,
    ParsedEvent, ResponseRoute,
};

use crate::multiplexer::types::PendingPrompt;
use crate::service::ServiceContext;

#[derive(Clone)]
struct Active {
    prompt: PendingPrompt,
    route: ResponseRoute,
    in_flight: bool,
}

#[derive(Default)]
struct ChatStreamState {
    session_id: Option<String>,
    messages: HashMap<String, MessageMeta>,
    part_text: HashMap<String, String>,
    part_kind: HashMap<String, String>,
    recovered_parts: HashSet<String>,
}

struct ChatBinding {
    session_id: String,
    binding_nonce: Option<String>,
}

static ACTIVE: LazyLock<TimedMutex<HashMap<String, Active>>> =
    LazyLock::new(|| TimedMutex::new("structured_prompt_active", HashMap::new()));
static QUEUED: LazyLock<TimedMutex<HashMap<String, Vec<Active>>>> =
    LazyLock::new(|| TimedMutex::new("structured_prompt_queued", HashMap::new()));
struct Subscriber {
    binding: subscription::Binding,
    signal: tokio::sync::watch::Sender<subscriber::Wake>,
    live: Arc<Mutex<bool>>,
}

static SUBSCRIBED: LazyLock<TimedMutex<HashMap<String, Subscriber>>> =
    LazyLock::new(|| TimedMutex::new("structured_prompt_subscribed", HashMap::new()));

/// Allocate a free loopback port for a provider's structured-prompt server by
/// binding `127.0.0.1:0`, reading the assigned port, then releasing it so the
/// agent can bind it. A small TOCTOU window, acceptable for a local dev server.
pub fn allocate_server() -> Option<crate::agent::provider::StructuredPromptServer> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).ok()?;
    let port = listener.local_addr().ok()?.port();
    drop(listener);
    Some(crate::agent::provider::StructuredPromptServer {
        hostname: "127.0.0.1".to_string(),
        port,
    })
}

/// Record the structured-prompt server for a tab (called at spawn, once the
/// host has allocated the fixed port). `base_url` is e.g. `http://127.0.0.1:PORT`.
pub fn register_endpoint(tab_id: &str, base_url: String, cwd: String) {
    stop_subscriber(tab_id);
    super::structured_endpoint::register(
        tab_id,
        super::structured_endpoint::Endpoint::Sse { base_url, cwd },
    );
}

fn endpoint_for_tab(tab_id: &str) -> Option<(String, String)> {
    super::structured_endpoint::sse(tab_id)
}

/// The live structured prompt for a tab, if one is pending. Cheap in-memory
/// read on the hot path.
pub fn pending_for_tab(tab_id: &str) -> Option<PendingPrompt> {
    if let Some(prompt) = crate::service::permission_decision::pending_for_tab(tab_id) {
        return Some(prompt);
    }
    crate::service::codex_rpc::approvals::pending(tab_id)
        .or_else(|| ACTIVE.lock().get(tab_id).map(|a| a.prompt.clone()))
}

#[cfg(test)]
fn route_for_tab(tab_id: &str) -> Option<ResponseRoute> {
    ACTIVE.lock().get(tab_id).map(|a| a.route.clone())
}

fn clear_request(active: &mut HashMap<String, Active>, tab_id: &str, request_id: &str) -> bool {
    if active
        .get(tab_id)
        .is_some_and(|entry| entry.route.request_id == request_id)
    {
        active.remove(tab_id);
        true
    } else {
        false
    }
}

/// Drop all structured-prompt state for a tab (tab closed / agent exited).
pub fn clear_tab(tab_id: &str) {
    crate::service::permission_decision::clear_tab(tab_id);
    crate::service::codex_rpc::launch::clear_tab(tab_id);
    let subscriber = SUBSCRIBED.lock().remove(tab_id);
    if let Some(subscriber) = subscriber {
        *subscriber.live.lock() = false;
    }
    if crate::service::codex_rpc::lifecycle::for_tab(tab_id).is_none() {
        super::structured_endpoint::remove(tab_id);
    }
    ACTIVE.lock().remove(tab_id);
    QUEUED.lock().remove(tab_id);
}

fn stop_subscriber(tab_id: &str) {
    let subscriber = { SUBSCRIBED.lock().remove(tab_id) };
    if let Some(subscriber) = subscriber {
        *subscriber.live.lock() = false;
    }
}

pub(crate) fn bind_launch(tab_id: &str, nonce: &str) {
    stop_subscriber(tab_id);
    super::structured_endpoint::bind_sse_launch(tab_id, nonce);
}

/// Start the SSE subscriber for a tab if it has a registered endpoint and none
/// is running yet. Idempotent — safe to call on every session-bind. The caller
/// has already gated on `supports_structured_prompts`.
pub fn maybe_start_subscriber(svc: &ServiceContext, tab_id: &str) {
    let Some(binding) = subscription::current(svc, tab_id) else {
        return;
    };
    let mut subscribed = SUBSCRIBED.lock();
    if let Some(subscriber) = subscribed.get(tab_id) {
        if subscriber.binding == binding {
            subscriber
                .signal
                .send_modify(|wake| wake.output = wake.output.wrapping_add(1));
            return;
        }
    }
    let previous = subscribed.remove(tab_id);
    let (signal, receiver) = tokio::sync::watch::channel(subscriber::Wake::default());
    let live = Arc::new(Mutex::new(true));
    subscribed.insert(
        tab_id.to_string(),
        Subscriber {
            binding: binding.clone(),
            signal,
            live: live.clone(),
        },
    );
    drop(subscribed);
    if let Some(previous) = previous {
        *previous.live.lock() = false;
    }
    log::info!("[structured-channel] tab={tab_id} status=subscribing endpoint={} session={} binding_nonce={}", binding.base_url, binding.session_id, binding.nonce);
    let svc = svc.clone();
    let tab_id = tab_id.to_string();
    crate::async_runtime::spawn(subscriber::run(svc, tab_id, binding, receiver, live));
}

/// Allocate the server; subscription starts after authenticated launch binding.
pub fn ensure_for_spawn(
    svc: &ServiceContext,
    agent_cmd: &str,
    tab_id: &str,
    cwd: &str,
) -> Option<crate::agent::provider::StructuredPromptServer> {
    let needs = svc
        .agent_registry
        .with_provider(agent_cmd, |p| p.needs_structured_prompt_server())
        .unwrap_or(false);
    if !needs {
        return None;
    }
    let server = allocate_server()?;
    let base_url = format!("http://{}:{}", server.hostname, server.port);
    register_endpoint(tab_id, base_url, cwd.to_string());
    let nonce = svc.ensure_session_binding_nonce(tab_id);
    bind_launch(tab_id, &nonce);
    Some(server)
}

/// Answer a pending structured prompt over HTTP. Returns `Some(result)` when the
/// tab has a structured route (the PTY send-back must then be skipped), or
/// `None` when there is no structured handle (caller falls back to PTY).
pub(crate) enum StructuredResponse {
    Advanced,
    Answered { option_index: usize },
}

pub(crate) fn respond(
    svc: &ServiceContext,
    tab_id: &str,
    value: &str,
) -> Option<Result<StructuredResponse, String>> {
    if let Some(result) =
        crate::service::permission_decision::respond_with_identity(svc, tab_id, value, None)
    {
        return Some(result.map(|()| StructuredResponse::Advanced));
    }
    if let Some(result) = crate::service::codex_rpc::approvals::respond(svc, tab_id, value) {
        return Some(result);
    }
    let mut active = ACTIVE.lock();
    let entry = active.get_mut(tab_id)?;
    if entry.in_flight {
        return Some(Err("structured reply already in flight".into()));
    }
    let decision = match questions::answer(&entry.route, value) {
        Some(decision) => decision,
        None => return Some(Err("invalid structured answer".into())),
    };
    let (body, option_index) = match decision {
        questions::Answer::Next { route, prompt } => {
            entry.route = route;
            entry.prompt = prompt.clone();
            drop(active);
            svc.push_structured_prompt(tab_id, Some(prompt));
            return Some(Ok(StructuredResponse::Advanced));
        }
        questions::Answer::Complete { body, option_index } => (body, option_index),
    };
    let route = entry.route.clone();
    entry.in_flight = true;
    drop(active);
    let Some((base_url, cwd)) = endpoint_for_tab(tab_id) else {
        if let Some(entry) = ACTIVE.lock().get_mut(tab_id) {
            entry.in_flight = false;
        }
        return Some(Err("structured endpoint disappeared".into()));
    };
    let result = send_reply(&base_url, &cwd, &route, &body);
    let mut active = ACTIVE.lock();
    let unchanged = active.get(tab_id).is_some_and(|entry| entry.route == route);
    if unchanged {
        if result.is_ok() {
            active.remove(tab_id);
        } else if let Some(entry) = active.get_mut(tab_id) {
            entry.in_flight = false;
        }
    }
    drop(active);
    if unchanged && result.is_ok() {
        svc.push_structured_prompt(tab_id, promote_prompt(tab_id));
    }
    if result.is_ok() {
        request_reconcile(tab_id);
    }
    Some(result.map(|()| StructuredResponse::Answered { option_index }))
}

fn request_reconcile(tab_id: &str) {
    if let Some(signal) = SUBSCRIBED.lock().get(tab_id) {
        signal
            .signal
            .send_modify(|wake| wake.snapshot = wake.snapshot.wrapping_add(1));
    }
}

pub fn abort_session(
    svc: &ServiceContext,
    tab_id: &str,
    session_id: &str,
) -> Option<Result<(), String>> {
    let (base_url, _) = endpoint_for_tab(tab_id)?;
    let url = format!("{base_url}/session/{session_id}/abort");
    let result = abort_session_request(&url);
    if result.is_ok() {
        ACTIVE.lock().remove(tab_id);
        svc.push_structured_prompt(tab_id, None);
    }
    Some(result)
}

fn abort_session_request(url: &str) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.post(url).send().map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("structured abort {}: HTTP {}", url, resp.status()))
    }
}

fn send_reply(
    base_url: &str,
    cwd: &str,
    route: &ResponseRoute,
    body: &serde_json::Value,
) -> Result<(), String> {
    let url = format!("{base_url}{}", reply_endpoint(route));
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(&url)
        .query(&[("directory", cwd)])
        .json(body)
        .send()
        .map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("opencode reply {}: HTTP {}", url, resp.status()))
    }
}

pub(crate) fn wake_for_tab(tab_id: &str) {
    if let Some(signal) = SUBSCRIBED.lock().get(tab_id) {
        signal
            .signal
            .send_modify(|wake| wake.output = wake.output.wrapping_add(1));
    }
}

fn handle_event(
    svc: &ServiceContext,
    tab_id: &str,
    value: &serde_json::Value,
    chat_state: &mut ChatStreamState,
) {
    match parse_event(value) {
        ParsedEvent::SessionUpdate { session_id, kind } => {
            if !owns_session(svc, tab_id, &session_id) {
                return;
            }
            let nonce = svc
                .mux
                .read()
                .get_tab(tab_id)
                .and_then(|tab| tab.session_binding_nonce.clone());
            crate::service::tab_chat::structured_sink::emit_chat_msgs(
                svc.ui.as_ref(),
                tab_id,
                &[session_events::message(&session_id, &kind)],
                None,
                Some(&session_id),
                nonce.as_deref(),
            );
            match kind {
                opencode_sse::SessionUpdate::Compacted => request_reconcile(tab_id),
                opencode_sse::SessionUpdate::Error { .. } => {
                    crate::agent::state_verdict::observe_declared_event(
                        svc,
                        tab_id,
                        &session_id,
                        "session.error",
                    );
                    svc.on_agent_turn_settled(tab_id);
                    request_reconcile(tab_id);
                }
                opencode_sse::SessionUpdate::Todos { .. } => {}
            }
        }
        ParsedEvent::MessageUpdated { message } => {
            if chat_binding(svc, tab_id, &message.session_id).is_some() {
                observe_message_model(svc, tab_id, &message);
                chat_state.record_message(message);
            }
        }
        ParsedEvent::PartUpdated { update } => {
            emit_chat_part(svc, tab_id, &update, chat_state);
        }
        ParsedEvent::Prompt { prompt, route } => {
            if !owns_session(svc, tab_id, &route.session_id) {
                return;
            }
            let mut active = ACTIVE.lock();
            let mut queued = QUEUED.lock();
            let duplicate = active
                .get(tab_id)
                .is_some_and(|entry| entry.route.request_id == route.request_id)
                || queued.get(tab_id).is_some_and(|entries| {
                    entries
                        .iter()
                        .any(|entry| entry.route.request_id == route.request_id)
                });
            if duplicate {
                return;
            }
            let entry = Active {
                prompt: prompt.clone(),
                route,
                in_flight: false,
            };
            if active.contains_key(tab_id) {
                queued.entry(tab_id.into()).or_default().push(entry);
                return;
            }
            active.insert(tab_id.into(), entry);
            drop(queued);
            drop(active);
            svc.push_structured_prompt(tab_id, Some(prompt));
        }
        ParsedEvent::Replied { request_id } => {
            let cleared = clear_request(&mut ACTIVE.lock(), tab_id, &request_id);
            if let Some(queued) = QUEUED.lock().get_mut(tab_id) {
                queued.retain(|entry| entry.route.request_id != request_id);
            }
            if cleared {
                svc.push_structured_prompt(tab_id, promote_prompt(tab_id));
                request_reconcile(tab_id);
            }
        }
        // The bus already carries the provider's own turn state; what it
        // declares in its roster is a verdict, not another ignored event.
        ParsedEvent::Ignored => {
            let event = value.get("type").and_then(|t| t.as_str());
            let session_id = value
                .get("properties")
                .and_then(|properties| properties.get("sessionID"))
                .and_then(|session_id| session_id.as_str());
            if let (Some(event), Some(session_id)) = (event, session_id) {
                crate::agent::state_verdict::observe_declared_event(svc, tab_id, session_id, event);
                crate::service::public_send_readiness::observe_bootstrap_turn_idle(
                    svc, tab_id, event, session_id,
                );
            }
        }
    }
}

fn observe_message_model(svc: &ServiceContext, tab_id: &str, message: &MessageMeta) {
    let Some(model) = message.model.as_ref() else {
        return;
    };
    let identity = {
        let mux = svc.mux.read();
        mux.get_tab(tab_id).and_then(|tab| {
            if tab.agent_session_id.as_deref() != Some(&message.session_id) {
                return None;
            }
            Some((tab.agent_type.clone()?, tab.session_binding_nonce.clone()?))
        })
    };
    if let Some((provider, nonce)) = identity {
        svc.apply_session_model(
            tab_id,
            &provider,
            &message.session_id,
            model.clone(),
            None,
            Some(&nonce),
            false,
        );
    }
}

impl ChatStreamState {
    fn reset_session(&mut self, session_id: &str) {
        if self.session_id.as_deref() == Some(session_id) {
            return;
        }
        self.session_id = Some(session_id.to_string());
        self.messages.clear();
        self.clear_part_state();
    }

    fn record_message(&mut self, message: MessageMeta) {
        self.reset_session(&message.session_id);
        if self.messages.len() >= 512 && !self.messages.contains_key(&message.message_id) {
            self.messages.clear();
            self.clear_part_state();
        }
        self.messages.insert(message.message_id.clone(), message);
    }

    fn clear_part_state(&mut self) {
        self.part_text.clear();
        self.part_kind.clear();
        self.recovered_parts.clear();
    }

    fn clear_part_state_if_full(&mut self, part_id: &str, needs_text: bool, needs_kind: bool) {
        let text_full =
            needs_text && self.part_text.len() >= 512 && !self.part_text.contains_key(part_id);
        let kind_full =
            needs_kind && self.part_kind.len() >= 512 && !self.part_kind.contains_key(part_id);
        if text_full || kind_full {
            self.clear_part_state();
        }
    }

    fn record_part_kind(&mut self, update: &ChatPartUpdate) {
        if update.event != ChatPartEvent::Updated {
            return;
        }
        let Some(kind) = update.part.get("type").and_then(|value| value.as_str()) else {
            return;
        };
        self.clear_part_state_if_full(&update.part_id, false, true);
        self.part_kind
            .insert(update.part_id.clone(), kind.to_string());
    }

    fn prepare_chat_part(
        &mut self,
        update: &ChatPartUpdate,
        message: &MessageMeta,
    ) -> Option<crate::agent::chat_types::ChatMsg> {
        if update.session_id != message.session_id || update.message_id != message.message_id {
            return None;
        }

        self.record_part_kind(update);
        let kind = self.part_kind.get(&update.part_id).cloned();
        let is_text = matches!(kind.as_deref(), Some("text" | "reasoning"))
            || (kind.is_none() && update.delta.is_some());
        self.clear_part_state_if_full(&update.part_id, is_text, false);
        if let Some(kind) = kind.as_ref() {
            self.part_kind.insert(update.part_id.clone(), kind.clone());
        }

        let previous_text = self.part_text.get(&update.part_id).cloned();
        if kind.is_none() {
            let delta = update.delta.as_ref()?;
            self.part_text.insert(
                update.part_id.clone(),
                format!("{}{}", previous_text.as_deref().unwrap_or_default(), delta),
            );
            return None;
        }

        let mut typed_update = update.clone();
        if let Some(kind) = kind.as_ref() {
            typed_update.part["type"] = serde_json::Value::String(kind.clone());
        }
        let text_is_empty = update
            .part
            .get("text")
            .and_then(|value| value.as_str())
            .map_or(true, |text| text.trim().is_empty());
        if matches!(kind.as_deref(), Some("text" | "reasoning"))
            && update.event == ChatPartEvent::Updated
            && text_is_empty
        {
            if let Some(previous_text) = previous_text.as_ref() {
                typed_update.part["text"] = serde_json::Value::String(previous_text.clone());
            }
        }
        if !is_text {
            self.part_text.remove(&update.part_id);
        }

        let chat_message = normalize_chat_part(&typed_update, message, previous_text.as_deref())?;
        if is_text {
            self.part_text
                .insert(update.part_id.clone(), chat_message.content.clone());
        }
        Some(chat_message)
    }
}

fn chat_binding(svc: &ServiceContext, tab_id: &str, session_id: &str) -> Option<ChatBinding> {
    let (provider_id, bound_session, binding_nonce) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id)?;
        (
            tab.agent_type.clone()?,
            tab.agent_session_id.clone()?,
            tab.session_binding_nonce.clone(),
        )
    };
    if bound_session != session_id {
        return None;
    }
    let supports_chat = svc
        .agent_registry
        .with_provider(&provider_id, |provider| provider.uses_structured_chat())
        .unwrap_or(false);
    supports_chat.then_some(ChatBinding {
        session_id: bound_session,
        binding_nonce,
    })
}

fn emit_chat_part(
    svc: &ServiceContext,
    tab_id: &str,
    update: &ChatPartUpdate,
    chat_state: &mut ChatStreamState,
) {
    let Some(binding) = chat_binding(svc, tab_id, &update.session_id) else {
        return;
    };
    chat_state.reset_session(&binding.session_id);
    chat_state.record_part_kind(update);
    let Some(message) = chat_state.messages.get(&update.message_id).cloned() else {
        return;
    };
    let Some(mut chat_message) = chat_state.prepare_chat_part(update, &message) else {
        return;
    };
    tool_diffs::apply(svc, tab_id, &update.part, &mut chat_message);
    let sink = crate::service::tab_chat::structured_sink::make_sink(
        svc.lifecycle_journal.clone(),
        &svc,
        svc.mux.clone(),
        Some(binding.session_id),
        binding.binding_nonce,
        Some(svc.chat_send_ledger.clone()),
    );
    sink.emit(tab_id, vec![chat_message], None);
}

fn owns_session(svc: &ServiceContext, tab_id: &str, session_id: &str) -> bool {
    svc.mux
        .read()
        .get_tab(tab_id)
        .is_some_and(|tab| tab.agent_session_id.as_deref() == Some(session_id))
}

fn promote_prompt(tab_id: &str) -> Option<PendingPrompt> {
    let mut active = ACTIVE.lock();
    if let Some(entry) = active.get(tab_id) {
        return Some(entry.prompt.clone());
    }
    let mut queued = QUEUED.lock();
    let entries = queued.get_mut(tab_id)?;
    if entries.is_empty() {
        return None;
    }
    let next = entries.remove(0);
    let prompt = next.prompt.clone();
    active.insert(tab_id.into(), next);
    Some(prompt)
}

pub use snapshot::{read as read_snapshot, RETAINED_ROWS};
