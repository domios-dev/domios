pub(super) fn may_be_directory_trust_prompt(screen: &str) -> bool {
    let tail = screen.lines().rev().take(24).collect::<Vec<_>>();
    let context = tail.join("\n").to_lowercase();
    let has_directory_question = context.contains('?')
        && ["folder", "directory", "workspace"]
            .iter()
            .any(|word| context.contains(*word))
        && ["trust", "trusted", "load"]
            .iter()
            .any(|word| context.contains(*word));
    let has_confirm_footer = tail.iter().any(|line| {
        let line = line.to_lowercase();
        line.contains("enter")
            && ["confirm", "continue", "select"]
                .iter()
                .any(|word| line.contains(*word))
    });
    let choices = tail
        .iter()
        .filter(|line| is_choice_row(line))
        .count();

    has_directory_question && has_confirm_footer && choices >= 2
}

pub(super) fn provider_classifies_directory_trust_prompt(
    registry: &crate::agent::registry::AgentRegistry,
    agent_type: &str,
    screen: &str,
) -> bool {
    may_be_directory_trust_prompt(screen)
        && registry
            .with_provider(agent_type, |provider| {
                provider.directory_trust_prompt_response(screen).is_some()
            })
            .unwrap_or(false)
}

pub(super) fn candidate_for_output(
    service: &crate::service::ServiceContext,
    tab_id: &str,
    agent_type: &str,
    screen: Option<&str>,
    provenance_applies: bool,
    auto_approve: bool,
    skip_reason: Option<&'static str>,
) -> bool {
    service.maybe_detect_agent_session(tab_id, agent_type);
    let candidate = if provenance_applies && screen.is_some() {
        service
            .respond_to_directory_trust(tab_id, screen)
            .owns_prompt()
    } else if auto_approve {
        screen.is_some_and(|screen| {
            provider_classifies_directory_trust_prompt(
                &service.agent_registry,
                agent_type,
                screen,
            )
        })
    } else {
        false
    };
    if !provenance_applies {
        if let Some(reason) = skip_reason {
            service.note_directory_trust(tab_id, reason, None);
        }
    }
    candidate
}

fn is_choice_row(line: &str) -> bool {
    let row = line
        .trim_start()
        .strip_prefix('│')
        .unwrap_or(line.trim_start())
        .trim_start();
    let row = row
        .strip_prefix('›')
        .or_else(|| row.strip_prefix('❯'))
        .or_else(|| row.strip_prefix('>'))
        .unwrap_or(row)
        .trim_start();
    let digits = row.chars().take_while(char::is_ascii_digit).count();
    if digits > 0
        && row[digits..]
            .chars()
            .next()
            .is_some_and(|separator| matches!(separator, '.' | ')'))
    {
        return true;
    }
    let normalized = row.to_lowercase();
    ["yes", "no", "trust", "quit"]
        .iter()
        .any(|choice| normalized.starts_with(choice))
}

#[cfg(test)]
#[path = "trust_candidate_tests.rs"]
mod tests;
