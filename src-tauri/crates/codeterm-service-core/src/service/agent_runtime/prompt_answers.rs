use super::{
    engine::{self, Session},
    model::{self, RuntimeFailure},
};
use crate::service::{tab::RespondPromptOutcome, ServiceContext};
use codeterm_plugin::agent_runtime::{PromptAnswer, RpcId, RuntimeContext, RuntimeOperation};
use codeterm_types::pty::{PendingPrompt, PromptIdentity, PromptKind};
use parking_lot::Mutex;
use std::{
    collections::{HashMap, VecDeque},
    sync::LazyLock,
};

#[derive(Clone)]
struct Entry {
    request: RpcId,
    epoch: u64,
    binding: String,
    thread: Option<String>,
    prompt: PendingPrompt,
    claimed: bool,
}

static PROMPTS: LazyLock<Mutex<HashMap<String, VecDeque<Entry>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn request_token(context: &RuntimeContext, request: &RpcId) -> String {
    serde_json::json!([context.epoch, context.binding_nonce, request]).to_string()
}

pub fn publish(svc: &ServiceContext, session: &Session, request: RpcId, mut prompt: PendingPrompt) {
    let context = session.context.lock().clone();
    super::client_bridge::approvals::remember(&context, &request);
    prompt.request_id = Some(request_token(&context, &request));
    let visible = {
        let mut prompts = PROMPTS.lock();
        let queue = prompts.entry(context.tab_id.clone()).or_default();
        queue.retain(|entry| {
            entry.epoch == context.epoch
                && entry.binding == context.binding_nonce
                && entry.thread == context.thread_id
        });
        if let Some(entry) = queue.iter_mut().find(|entry| entry.request == request) {
            entry.prompt = prompt;
        } else {
            queue.push_back(Entry {
                request,
                epoch: context.epoch,
                binding: context.binding_nonce,
                thread: context.thread_id,
                prompt,
                claimed: false,
            });
        }
        queue.front().map(|entry| entry.prompt.clone())
    };
    svc.push_structured_prompt(&context.tab_id, visible);
    svc.mark_agent_waiting(&context.tab_id);
    auto_confirm_bridge(svc, &context.tab_id);
}

pub fn resolve(svc: &ServiceContext, session: &Session, request: &RpcId) {
    let context = session.context.lock().clone();
    remove(svc, &context, Some(request));
}

pub fn clear(svc: &ServiceContext, context: &RuntimeContext) {
    remove(svc, context, None);
}

fn remove(svc: &ServiceContext, context: &RuntimeContext, request: Option<&RpcId>) {
    super::client_bridge::approvals::forget(context, request);
    let (removed, next) = {
        let mut prompts = PROMPTS.lock();
        let Some(queue) = prompts.get_mut(&context.tab_id) else {
            return;
        };
        let before = queue.front().map(|entry| entry.prompt.clone());
        queue.retain(|entry| {
            !(entry.epoch == context.epoch
                && entry.binding == context.binding_nonce
                && entry.thread == context.thread_id
                && request.is_none_or(|request| &entry.request == request))
        });
        let next = queue.front().map(|entry| entry.prompt.clone());
        if queue.is_empty() {
            prompts.remove(&context.tab_id);
        }
        (before, next)
    };
    let current = svc
        .mux
        .read()
        .get_tab(&context.tab_id)
        .and_then(|tab| tab.pending_prompt.clone());
    if removed != next && current == removed {
        if let Some(prompt) = removed.as_ref() {
            svc.resolve_prompt_notifications(
                &context.tab_id,
                &PromptIdentity::new(PromptKind::Structured, prompt),
            );
        }
        svc.push_structured_prompt(&context.tab_id, next);
        auto_confirm_bridge(svc, &context.tab_id);
    }
}

fn matching_entry(entry: &Entry, context: &RuntimeContext, expected: &PromptIdentity) -> bool {
    !entry.claimed
        && entry.epoch == context.epoch
        && entry.binding == context.binding_nonce
        && entry.thread == context.thread_id
        && entry.prompt.request_id.as_deref()
            == Some(request_token(context, &entry.request).as_str())
        && PromptIdentity::new(PromptKind::Structured, &entry.prompt) == *expected
}

fn current_prompt_matches(
    svc: &ServiceContext,
    tab: &str,
    context: &RuntimeContext,
    expected: &PromptIdentity,
) -> bool {
    svc.mux.read().get_tab(tab).is_some_and(|tab| {
        tab.session_binding_nonce.as_deref() == Some(context.binding_nonce.as_str())
            && tab.reported_session_binding_nonce.as_deref() == Some(context.binding_nonce.as_str())
            && tab.agent_session_id == context.thread_id
            && tab.prompt_identity().as_ref() == Some(expected)
    })
}

fn answer(prompt: &PendingPrompt, value: &str) -> Result<PromptAnswer, RuntimeFailure> {
    let answer = if prompt.options.is_empty() {
        PromptAnswer::Text { text: value.into() }
    } else if prompt.multi_select == Some(true) {
        let values = if value.starts_with('[') {
            serde_json::from_str::<Vec<String>>(value)
                .map_err(|error| RuntimeFailure::Refused(error.to_string()))?
        } else {
            let mask = value
                .parse::<u64>()
                .ok()
                .and_then(|value| value.checked_sub(10000))
                .ok_or_else(|| RuntimeFailure::Refused("Invalid prompt selection".into()))?;
            let mut known = 0;
            let mut selected = Vec::new();
            for option in &prompt.options {
                let index = option
                    .value
                    .parse::<u32>()
                    .ok()
                    .and_then(|value| value.checked_sub(1))
                    .filter(|index| *index < 31)
                    .ok_or_else(|| {
                        RuntimeFailure::Refused("Prompt requires native values".into())
                    })?;
                let bit = 1_u64 << index;
                known |= bit;
                if mask & bit != 0 {
                    selected.push(option.value.clone());
                }
            }
            if mask & !known != 0 {
                return Err(RuntimeFailure::Refused("Unknown prompt selection".into()));
            }
            selected
        };
        PromptAnswer::Choice { values }
    } else {
        let option = prompt
            .options
            .iter()
            .find(|option| option.value == value)
            .or_else(|| {
                value
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| index.checked_sub(1))
                    .and_then(|index| prompt.options.get(index))
            })
            .ok_or_else(|| RuntimeFailure::Refused("Unknown prompt option".into()))?;
        PromptAnswer::Choice {
            values: vec![option.value.clone()],
        }
    };
    model::validate_prompt_answer(prompt, &answer)?;
    Ok(answer)
}

pub async fn respond(
    svc: &ServiceContext,
    tab: &str,
    value: &str,
    expected: Option<&PromptIdentity>,
) -> Option<Result<RespondPromptOutcome, String>> {
    respond_inner(svc, tab, value, expected, false).await
}

pub(crate) fn auto_confirm_bridge(svc: &ServiceContext, tab: &str) {
    if !automatic_bridge_enabled(svc, tab) {
        return;
    }
    let entry = PROMPTS
        .lock()
        .get(tab)
        .and_then(|queue| queue.front())
        .filter(|entry| !entry.claimed)
        .cloned();
    let Some(entry) = entry else {
        return;
    };
    let Some(session) = engine::for_tab(tab) else {
        return;
    };
    let context = session.context.lock().clone();
    if !super::client_bridge::pending(&context, &entry.request) {
        return;
    }
    let identity = PromptIdentity::new(PromptKind::Structured, &entry.prompt);
    let owner = svc.clone();
    let tab = tab.to_owned();
    crate::async_runtime::spawn(async move {
        let _ = respond_inner(&owner, &tab, "allow_once", Some(&identity), true).await;
    });
}

/// Requests already waiting on the owner follow a changed permission setting.
pub(super) fn settle_after_policy_change(svc: &ServiceContext, tab: &str) {
    let Some(session) = engine::for_tab(tab) else {
        return;
    };
    let waiting: Vec<(RpcId, PendingPrompt)> = PROMPTS
        .lock()
        .get(tab)
        .map(|queue| {
            queue
                .iter()
                .filter(|entry| !entry.claimed)
                .map(|entry| (entry.request.clone(), entry.prompt.clone()))
                .collect()
        })
        .unwrap_or_default();
    for (request, prompt) in waiting {
        let permission = session
            .prompts
            .lock()
            .get(&request)
            .and_then(|pending| pending.permission.clone());
        super::permission_policy::answer(svc, &session, &request, &prompt, permission.as_ref());
    }
    auto_confirm_bridge(svc, tab);
}

fn automatic_bridge_enabled(svc: &ServiceContext, tab: &str) -> bool {
    svc.mux
        .read()
        .get_tab(tab)
        .is_some_and(|tab| tab.auto_approve || tab.permission_auto_confirm)
}

async fn respond_inner(
    svc: &ServiceContext,
    tab: &str,
    value: &str,
    expected: Option<&PromptIdentity>,
    automatic_bridge: bool,
) -> Option<Result<RespondPromptOutcome, String>> {
    let runtime = super::requested_for_tab(svc, tab);
    let has_token = expected.is_some_and(|identity| identity.request_id.is_some());
    if !runtime && !has_token {
        return None;
    }
    Some(
        async {
            let Some(session) = engine::for_tab(tab) else {
                return Ok(RespondPromptOutcome::StalePrompt);
            };
            let captured = session.context.lock().clone();
            let entry = PROMPTS
                .lock()
                .get(tab)
                .and_then(|queue| queue.front().cloned());
            let Some(entry) = entry else {
                return Ok(RespondPromptOutcome::StalePrompt);
            };
            let snapshot_identity = PromptIdentity::new(PromptKind::Structured, &entry.prompt);
            let expected = expected.unwrap_or(&snapshot_identity);
            if !matching_entry(&entry, &captured, expected)
                || !current_prompt_matches(svc, tab, &captured, expected)
            {
                return Ok(RespondPromptOutcome::StalePrompt);
            }
            let answer = match answer(&entry.prompt, value) {
                Ok(answer) => answer,
                Err(_) => return Ok(RespondPromptOutcome::InvalidOption),
            };
            let _permit =
                tokio::time::timeout(std::time::Duration::from_secs(30), session.gate.acquire())
                    .await
                    .map_err(|_| "Runtime approval permit deadline")?
                    .map_err(|_| "Runtime stopped")?;
            let current = session.context.lock().clone();
            if automatic_bridge
                && (!automatic_bridge_enabled(svc, tab)
                    || !super::client_bridge::pending(&current, &entry.request))
            {
                return Ok(RespondPromptOutcome::StalePrompt);
            }
            if !matching_entry(&entry, &current, expected)
                || !current_prompt_matches(svc, tab, &current, expected)
                || engine::validate_binding(svc, &session).is_err()
                || !session.prompts.lock().contains_key(&entry.request)
            {
                return Ok(RespondPromptOutcome::StalePrompt);
            }
            let process = session
                .process
                .lock()
                .clone()
                .ok_or("Runtime process unavailable")?;
            {
                let mut prompts = PROMPTS.lock();
                let Some(live) = prompts.get_mut(tab).and_then(|queue| queue.front_mut()) else {
                    return Ok(RespondPromptOutcome::StalePrompt);
                };
                if !matching_entry(live, &current, expected) {
                    return Ok(RespondPromptOutcome::StalePrompt);
                }
                live.claimed = true;
            }
            let bridged =
                super::client_bridge::answer(svc, &session, &process, &entry.request, &answer)
                    .await
                    .map_err(|error| error.to_string())?;
            if bridged {
                return Ok(RespondPromptOutcome::Structured);
            }
            let result = engine::run(
                svc,
                &session.provider,
                &session,
                &process,
                RuntimeOperation::Answer {
                    request_id: entry.request.clone(),
                    answer,
                },
            )
            .await;
            if !session.prompts.lock().contains_key(&entry.request) {
                remove(svc, &captured, Some(&entry.request));
            }
            if let Err(error) = result {
                session.state.lock().degrade(captured.epoch, &error);
                clear(svc, &captured);
                engine::publish(svc, tab, &session);
                return Err(error.to_string());
            }
            Ok(RespondPromptOutcome::Structured)
        }
        .await,
    )
}

pub fn respond_blocking(
    svc: &ServiceContext,
    tab: &str,
    value: &str,
    expected: Option<&PromptIdentity>,
) -> Option<Result<RespondPromptOutcome, String>> {
    if !super::requested_for_tab(svc, tab)
        && !expected.is_some_and(|identity| identity.request_id.is_some())
    {
        return None;
    }
    let owner = svc.clone();
    let tab = tab.to_string();
    let value = value.to_string();
    let expected = expected.cloned();
    match super::blocking::task(
        async move { respond(&owner, &tab, &value, expected.as_ref()).await },
    ) {
        Ok(result) => result,
        Err(error) => Some(Err(error)),
    }
}

pub(super) async fn cancel_pending(
    svc: &ServiceContext,
    session: &std::sync::Arc<Session>,
    process: &std::sync::Arc<super::process::Process>,
) -> Result<(), RuntimeFailure> {
    let mut context = session.context.lock().clone();
    let Some(adapter) = session.adapter.lock().clone() else {
        clear(svc, &context);
        return Ok(());
    };
    if context.client_bridges.is_none()
        && adapter
            .agent_runtime()
            .is_none_or(|cap| cap.client_bridges.is_none())
    {
        return Ok(());
    }
    let pending = std::mem::take(&mut *session.prompts.lock());
    let mut errors = Vec::new();
    for (request_id, pending) in pending {
        context.pending_request = Some(pending.request);
        let operation = RuntimeOperation::Answer {
            request_id: request_id.clone(),
            answer: PromptAnswer::Cancel,
        };
        let plan = tokio::time::timeout(
            codeterm_plugin::agent_runtime::RUNTIME_HOOK_DEADLINE,
            adapter.runtime_plan(&context, &operation),
        )
        .await;
        let result = match plan {
            Ok(Ok(codeterm_plugin::agent_runtime::RuntimePlan {
                action:
                    codeterm_plugin::agent_runtime::RuntimeAction::Reply {
                        request_id: id,
                        result,
                    },
                ..
            })) if id == request_id => {
                process
                    .client
                    .reply(
                        &id,
                        match result {
                            codeterm_plugin::agent_runtime::RpcReply::Success { value } => {
                                Ok(value)
                            }
                            codeterm_plugin::agent_runtime::RpcReply::Error { error } => Err(error),
                        },
                    )
                    .await
            }
            _ => Err(RuntimeFailure::Refused(
                "Pending request cancellation was refused".into(),
            )),
        };
        if let Err(error) = result {
            errors.push(error.to_string());
        }
        resolve(svc, session, &request_id);
    }
    clear(svc, &context);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(RuntimeFailure::Refused(errors.join("; ")))
    }
}

#[cfg(test)]
mod tests;
