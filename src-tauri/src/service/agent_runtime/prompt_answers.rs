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
    prompt.request_id = Some(request_token(&context, &request));
    let visible = {
        let mut prompts = PROMPTS.lock();
        let queue = prompts.entry(context.tab_id.clone()).or_default();
        queue
            .retain(|entry| entry.epoch == context.epoch && entry.binding == context.binding_nonce);
        if let Some(entry) = queue.iter_mut().find(|entry| entry.request == request) {
            entry.prompt = prompt;
        } else {
            queue.push_back(Entry {
                request,
                epoch: context.epoch,
                binding: context.binding_nonce,
                prompt,
                claimed: false,
            });
        }
        queue.front().map(|entry| entry.prompt.clone())
    };
    svc.push_structured_prompt(&context.tab_id, visible);
    svc.mark_agent_waiting(&context.tab_id);
}

pub fn resolve(svc: &ServiceContext, session: &Session, request: &RpcId) {
    let context = session.context.lock().clone();
    remove(svc, &context, Some(request));
}

pub fn clear(svc: &ServiceContext, context: &RuntimeContext) {
    remove(svc, context, None);
}

fn remove(svc: &ServiceContext, context: &RuntimeContext, request: Option<&RpcId>) {
    let (removed, next) = {
        let mut prompts = PROMPTS.lock();
        let Some(queue) = prompts.get_mut(&context.tab_id) else {
            return;
        };
        let before = queue.front().map(|entry| entry.prompt.clone());
        queue.retain(|entry| {
            !(entry.epoch == context.epoch
                && entry.binding == context.binding_nonce
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
    }
}

fn matching_entry(entry: &Entry, context: &RuntimeContext, expected: &PromptIdentity) -> bool {
    !entry.claimed
        && entry.epoch == context.epoch
        && entry.binding == context.binding_nonce
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

pub fn respond(
    svc: &ServiceContext,
    tab: &str,
    value: &str,
    expected: Option<&PromptIdentity>,
) -> Option<Result<RespondPromptOutcome, String>> {
    let runtime = super::requested_for_tab(svc, tab);
    let has_token = expected.is_some_and(|identity| identity.request_id.is_some());
    if !runtime && !has_token {
        return None;
    }
    Some((|| {
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
            super::blocking::deadline(std::time::Duration::from_secs(30), session.gate.acquire())
                .map_err(|_| "Runtime approval permit deadline")?
                .map_err(|_| "Runtime stopped")?;
        let current = session.context.lock().clone();
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
        let result = tauri::async_runtime::block_on(engine::run(
            svc,
            &session.provider,
            &session,
            &process,
            RuntimeOperation::Answer {
                request_id: entry.request.clone(),
                answer,
            },
        ));
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
    })())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prompt() -> PendingPrompt {
        serde_json::from_value(serde_json::json!({"question":"opaque", "options":[{"label":"Allow", "value":"native-yes"},{"label":"Reject", "value":"native-no"}]})).unwrap()
    }

    #[test]
    fn choices_send_native_values_and_reject_unknown_options() {
        assert_eq!(
            answer(&prompt(), "1").unwrap(),
            PromptAnswer::Choice {
                values: vec!["native-yes".into()]
            }
        );
        assert_eq!(
            answer(&prompt(), "native-no").unwrap(),
            PromptAnswer::Choice {
                values: vec!["native-no".into()]
            }
        );
        assert!(answer(&prompt(), "foreign").is_err());
    }

    #[test]
    fn multiselect_keeps_opaque_values_and_refuses_foreign_values() {
        let mut prompt = prompt();
        prompt.multi_select = Some(true);
        assert_eq!(
            answer(&prompt, r#"["native-yes","native-no"]"#).unwrap(),
            PromptAnswer::Choice {
                values: vec!["native-yes".into(), "native-no".into()]
            }
        );
        assert!(answer(&prompt, r#"["other"]"#).is_err());
        assert!(answer(&prompt, "10001").is_err());
    }
    #[test]
    fn exact_request_identity_rejects_epoch_binding_and_typed_id_changes() {
        let context = RuntimeContext {
            epoch: 2,
            binding_nonce: "own".into(),
            ..Default::default()
        };
        let id = RpcId::Number(7);
        let mut prompt = prompt();
        prompt.request_id = Some(request_token(&context, &id));
        let identity = PromptIdentity::new(PromptKind::Structured, &prompt);
        let mut entry = Entry {
            request: id,
            epoch: 2,
            binding: "own".into(),
            prompt,
            claimed: false,
        };
        assert!(matching_entry(&entry, &context, &identity));
        assert!(!matching_entry(
            &entry,
            &RuntimeContext {
                epoch: 3,
                ..context.clone()
            },
            &identity
        ));
        assert!(!matching_entry(
            &entry,
            &RuntimeContext {
                binding_nonce: "foreign".into(),
                ..context.clone()
            },
            &identity
        ));
        entry.request = RpcId::String("7".into());
        assert!(!matching_entry(&entry, &context, &identity));
        entry.request = RpcId::Number(8);
        assert!(!matching_entry(&entry, &context, &identity));
        entry.request = RpcId::Number(7);
        entry.claimed = true;
        assert!(!matching_entry(&entry, &context, &identity));
    }

    #[test]
    fn numeric_multiselect_rejects_unknown_bits_and_preserves_native_values() {
        let mut prompt = prompt();
        prompt.multi_select = Some(true);
        prompt.options[0].value = "1".into();
        prompt.options[1].value = "3".into();
        assert_eq!(
            answer(&prompt, "10005").unwrap(),
            PromptAnswer::Choice {
                values: vec!["1".into(), "3".into()]
            }
        );
        assert!(answer(&prompt, "10002").is_err());
    }
}
