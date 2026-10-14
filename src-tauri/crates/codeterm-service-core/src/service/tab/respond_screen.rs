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
    // ScreenState text is already rendered; parsing it directly preserves the observed row boundaries.
    let parsed = agent_type.and_then(|agent| {
        crate::agent::prompt::parse_prompt_safely(|| {
            registry.parse_interactive_prompt(agent, screen)
        })
    });
    if parsed
        .as_ref()
        .is_some_and(|(prompt, _)| prompt.multi_select == Some(true))
    {
        return Err(RespondPromptOutcome::InvalidOption);
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
            return Err(RespondPromptOutcome::StalePrompt);
        }
    }
    let Some((prompt, _)) = parsed else {
        return Err(RespondPromptOutcome::PromptNotOnScreen);
    };
    if option_index >= prompt.options.len() {
        return Err(RespondPromptOutcome::InvalidOption);
    }
    let steps = agent_type
        .and_then(|agent| registry.build_prompt_response(agent, option_index, screen))
        .unwrap_or_else(default_response);
    if steps.is_empty() {
        return Err(RespondPromptOutcome::PromptNotOnScreen);
    }
    Ok(steps)
}

#[cfg(test)]
mod tests;
