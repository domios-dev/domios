use super::RespondPromptOutcome;
use crate::agent::registry::AgentRegistry;
use codeterm_types::pty::{PromptIdentity, PromptKind};

pub(super) fn response_steps(
    registry: &AgentRegistry,
    agent_type: Option<&str>,
    screen: &str,
    option_index: usize,
    expected_identity: Option<&PromptIdentity>,
    default_response: impl FnOnce() -> Vec<Vec<u8>>,
) -> Result<Vec<Vec<u8>>, RespondPromptOutcome> {
    let parsed = agent_type.and_then(|agent| {
        crate::agent::prompt::parse_prompt_safely(|| {
            registry.parse_interactive_prompt(agent, screen)
        })
    });
    let observed = parsed
        .as_ref()
        .map(|(prompt, kind)| PromptIdentity::new(*kind, prompt));
    let refuse = |check: &str, outcome| {
        log::debug!("[respond_prompt-screen-refusal] check={} outcome={:?} option_index={} expected={:?} observed={:?}", check, outcome, option_index, expected_identity, observed);
        Err(outcome)
    };
    if parsed
        .as_ref()
        .is_some_and(|(prompt, _)| prompt.multi_select == Some(true))
    {
        return refuse("live_multi_select", RespondPromptOutcome::InvalidOption);
    }
    if let Some(expected) = expected_identity {
        let current = parsed
            .as_ref()
            .map(|(prompt, kind)| PromptIdentity::new(*kind, prompt));
        let manual_update_matches = expected.kind == PromptKind::ManualUpdate
            && current
                .as_ref()
                .is_some_and(|current| current.options == expected.options);
        if current.as_ref() != Some(expected) && !manual_update_matches {
            return refuse("live_identity", RespondPromptOutcome::StalePrompt);
        }
    }
    let Some((prompt, _)) = parsed else {
        return refuse(
            "live_parse_or_classification",
            RespondPromptOutcome::PromptNotOnScreen,
        );
    };
    if option_index >= prompt.options.len() {
        return refuse("live_option_bounds", RespondPromptOutcome::InvalidOption);
    }
    let steps = agent_type
        .and_then(|agent| registry.build_prompt_response(agent, option_index, screen))
        .unwrap_or_else(default_response);
    if steps.is_empty() {
        return refuse(
            "provider_keys_empty",
            RespondPromptOutcome::PromptNotOnScreen,
        );
    }
    Ok(steps)
}

#[cfg(test)]
mod tests;
