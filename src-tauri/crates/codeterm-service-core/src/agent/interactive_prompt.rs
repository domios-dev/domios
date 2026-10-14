use super::provider::AgentProvider;
use super::tab_state::{TabState, TabStateClassification, TabStateSnapshot};
use codeterm_types::pty::{PendingPrompt, PromptKind};

pub(crate) fn parse_live_prompt(
    provider: &dyn AgentProvider,
    screen: &str,
) -> Option<(PendingPrompt, PromptKind)> {
    let snapshot = TabStateSnapshot {
        tab_id: String::new(),
        agent_type: provider.id().to_string(),
        session_id: None,
        cwd: None,
        screen_text: screen.to_string(),
    };
    let classification = provider.classify_tab_state(&snapshot);
    let parsed = provider
        .parse_directory_trust_prompt(screen)
        .map(|prompt| (prompt, PromptKind::DirectoryTrust))
        .or_else(|| {
            provider
                .parse_prompt(screen)
                .map(|prompt| (prompt, PromptKind::Provider))
        });
    let resolved = resolve_live_prompt(parsed, classification.as_ref());
    log::trace!(
        "[live-prompt] provider={} classification={:?} interactive={}",
        provider.id(),
        classification.as_ref().map(|value| value.state),
        resolved.is_some()
    );
    resolved
}

fn resolve_live_prompt(
    parsed: Option<(PendingPrompt, PromptKind)>,
    classification: Option<&TabStateClassification>,
) -> Option<(PendingPrompt, PromptKind)> {
    let directory_trust = parsed
        .as_ref()
        .is_some_and(|(_, kind)| *kind == PromptKind::DirectoryTrust);
    let active = directory_trust
        || classification.is_none_or(|value| {
            value.confidence >= 0.8
                && matches!(
                    value.state,
                    TabState::WaitingOnAuth
                        | TabState::UpdatePrompt
                        | TabState::RateLimited
                        | TabState::ClarifyingQuestion
                )
        });
    active.then_some(parsed).flatten()
}

#[cfg(test)]
mod tests;
