use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::Mutex as StdMutex;

use super::ServiceContext;
use crate::agent::prompt::PromptDecision;

static RESPONDED_OCCURRENCES: LazyLock<StdMutex<HashMap<String, String>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

pub(crate) fn observation_revision(tab_id: &str) -> u64 {
    RESPONDED_OCCURRENCES
        .lock()
        .unwrap()
        .get(tab_id)
        .map_or(0, |occurrence| {
            super::super::supporter_tab_state::frame_hash(occurrence)
        })
}

pub(crate) fn claim_manual_update_occurrence(tab_id: &str, occurrence: &str) -> bool {
    let mut answered = RESPONDED_OCCURRENCES.lock().unwrap();
    if answered.get(tab_id).is_some_and(|seen| seen == occurrence) {
        return false;
    }
    answered.insert(tab_id.to_string(), occurrence.to_string());
    true
}

pub(crate) fn clear_manual_update_occurrence(tab_id: &str) {
    RESPONDED_OCCURRENCES.lock().unwrap().remove(tab_id);
}

pub(crate) fn release_model_update_step(tab_id: &str, occurrence: &str) {
    let mut answered = RESPONDED_OCCURRENCES.lock().unwrap();
    if answered
        .get(tab_id)
        .is_some_and(|current| current == occurrence)
    {
        answered.remove(tab_id);
    }
}

pub(crate) fn manual_update_prompt_candidate(screen: &str) -> bool {
    let mut options = Vec::new();
    let mut footer = false;
    for row in screen.lines() {
        let lower = row.to_ascii_lowercase();
        if lower.contains("enter")
            && ["confirm", "continue", "select"]
                .iter()
                .any(|word| lower.contains(word))
        {
            footer = true;
        }
        let row = row.trim_start();
        let row = row
            .strip_prefix('>')
            .or_else(|| row.strip_prefix('❯'))
            .or_else(|| row.strip_prefix('›'))
            .unwrap_or(row)
            .trim_start();
        let digits = row.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 {
            continue;
        }
        let Some(separator) = row[digits..].trim_start().chars().next() else {
            continue;
        };
        if !matches!(separator, '.' | ')') {
            continue;
        }
        let label = row[digits..]
            .trim_start()
            .get(separator.len_utf8()..)
            .unwrap_or_default()
            .trim_start()
            .to_ascii_lowercase();
        options.push(label);
    }
    footer && options.len() >= 2
}

#[cfg(test)]
pub(crate) fn parse_manual_update_prompt(
    provider: &dyn codeterm_plugin::provider::AgentProvider,
    snapshot: &codeterm_plugin::tab_state::TabStateSnapshot,
) -> Option<crate::multiplexer::types::PendingPrompt> {
    parse_manual_update_prompt_with_response(provider, snapshot).map(|(prompt, _)| prompt)
}

pub(crate) fn parse_manual_update_prompt_with_response(
    provider: &dyn codeterm_plugin::provider::AgentProvider,
    snapshot: &codeterm_plugin::tab_state::TabStateSnapshot,
) -> Option<(crate::multiplexer::types::PendingPrompt, Option<String>)> {
    if !manual_update_prompt_candidate(&snapshot.screen_text) {
        return None;
    }
    let classification = provider.classify_tab_state(snapshot);
    if !crate::agent::state_reactor::is_manual_update_prompt(classification.as_ref()) {
        return None;
    }
    let prompt = provider.parse_prompt(&snapshot.screen_text)?;
    let skip_value = provider.update_prompt_safe_choice(&prompt);
    Some((prompt, skip_value))
}

pub(crate) fn resolve_classified_manual_update_prompt(
    service: &ServiceContext,
    snapshot: &codeterm_plugin::tab_state::TabStateSnapshot,
    prompt: crate::multiplexer::types::PendingPrompt,
    skip_value: Option<String>,
) -> Option<crate::service::tab::RespondPromptOutcome> {
    resolve_classified_manual_update_prompt_fallback(service, snapshot, prompt, skip_value)
}

pub(crate) fn resolve_classified_manual_update_prompt_fallback(
    service: &ServiceContext,
    snapshot: &codeterm_plugin::tab_state::TabStateSnapshot,
    prompt: crate::multiplexer::types::PendingPrompt,
    skip_value: Option<String>,
) -> Option<crate::service::tab::RespondPromptOutcome> {
    if !agent_owns_prompt_answers(service, &snapshot.tab_id) {
        return resolve_manual_update_with_model_gate(service, snapshot, prompt, None, false);
    }
    crate::service::provider_update::manual_prompt_observed(service, snapshot);
    resolve_manual_update_with_model_gate(service, snapshot, prompt, skip_value, false)
}

fn agent_owns_prompt_answers(service: &ServiceContext, tab_id: &str) -> bool {
    service
        .mux
        .read()
        .get_tab(tab_id)
        .is_some_and(crate::service::unattended_launch::owns_prompt_answers)
}

pub(super) fn resolve_manual_update_with_model_gate(
    service: &ServiceContext,
    snapshot: &codeterm_plugin::tab_state::TabStateSnapshot,
    prompt: crate::multiplexer::types::PendingPrompt,
    skip_value: Option<String>,
    model_owned: bool,
) -> Option<crate::service::tab::RespondPromptOutcome> {
    let skip_value = if model_owned { None } else { skip_value };
    let decision = service.prompt_detector.lock().evaluate_pty_output_parsed(
        &snapshot.tab_id,
        true,
        true,
        Some(crate::agent::prompt::ResolvedPrompt {
            prompt: prompt.clone(),
            explicit: false,
        }),
    );
    let auto_response_declared = skip_value.is_some();
    if auto_response_declared {
        if !matches!(
            &decision,
            PromptDecision::NewPrompt { .. } | PromptDecision::PromptUpdated { .. }
        ) {
            return None;
        }
        let occurrence = serde_json::to_string(&prompt.options).unwrap_or_default();
        if !claim_manual_update_occurrence(&snapshot.tab_id, &occurrence) {
            return None;
        }
    }
    service.apply_prompt_decision_with_kind_and_routing(
        &decision,
        Some(&snapshot.screen_text),
        codeterm_types::pty::PromptKind::ManualUpdate,
        true,
        !auto_response_declared && !model_owned,
    );
    if let Some(value) = skip_value {
        let identity = codeterm_types::pty::PromptIdentity::new(
            codeterm_types::pty::PromptKind::ManualUpdate,
            &prompt,
        );
        let outcome =
            match service.respond_prompt_with_identity(&snapshot.tab_id, &value, &identity) {
                Ok(outcome) => outcome,
                Err(_) => {
                    clear_manual_update_occurrence(&snapshot.tab_id);
                    let _ = service.route_pending_prompt(
                        &snapshot.tab_id,
                        &prompt,
                        codeterm_types::pty::PromptKind::ManualUpdate,
                    );
                    return None;
                }
            };
        match outcome {
            crate::service::tab::RespondPromptOutcome::Sent
            | crate::service::tab::RespondPromptOutcome::Structured => {
                crate::service::provider_update::record_safe_prompt_answer(
                    &snapshot.agent_type,
                    &snapshot.tab_id,
                );
                if !crate::service::provider_update::coordinates_prompt(
                    service,
                    &snapshot.agent_type,
                ) {
                    let tab_title = service
                        .mux
                        .read()
                        .get_tab(&snapshot.tab_id)
                        .map(|tab| tab.title.clone())
                        .unwrap_or_else(|| snapshot.tab_id.clone());
                    service.create_notification_ext(
                        format!("{tab_title}: update skipped"),
                        Some("The classified update dialog was answered with Skip.".into()),
                        Some("info"),
                        None,
                        None,
                        super::super::notification::NotificationOrigin::local_tab(
                            snapshot.tab_id.clone(),
                        ),
                        super::super::notification::PushIntent {
                            level: Some(crate::notification::push::NotifyLevel::Verbose),
                            event_key: Some(format!(
                                "classified-update-skip:{}",
                                uuid::Uuid::new_v4()
                            )),
                        },
                        None,
                    );
                }
            }
            _ => {
                clear_manual_update_occurrence(&snapshot.tab_id);
                let _ = service.route_pending_prompt(
                    &snapshot.tab_id,
                    &prompt,
                    codeterm_types::pty::PromptKind::ManualUpdate,
                );
            }
        }
        Some(outcome)
    } else {
        None
    }
}
