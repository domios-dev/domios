fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn guidance_has_matching_persona(guidance: &str, persona: &str) -> bool {
    let Some(rest) = guidance.strip_prefix("You are ") else {
        return false;
    };
    let Some(rest) = rest.strip_prefix(persona) else {
        return false;
    };
    rest.is_empty()
        || rest.starts_with(|ch: char| {
            matches!(
                ch,
                '.' | ',' | ':' | ';' | '!' | '?' | '-' | ' ' | '\n' | '\r' | '\t'
            )
        })
}

pub fn compose_worker_identity(
    personality: Option<&str>,
    role_guidance: Option<&str>,
    task_name: Option<&str>,
) -> Option<String> {
    let personality = trimmed(personality);
    let role_guidance = trimmed(role_guidance);
    let task_name = trimmed(task_name);
    match (personality, role_guidance) {
        (Some(persona), Some(guidance)) => {
            if guidance_has_matching_persona(guidance, persona) {
                Some(guidance.to_string())
            } else {
                Some(format!("You are {persona}. {guidance}"))
            }
        }
        (None, Some(guidance)) => Some(guidance.to_string()),
        (Some(persona), None) => task_name
            .map(|task_name| format!("You are {persona} — executor for: {task_name}"))
            .or_else(|| Some(format!("You are {persona}."))),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::compose_worker_identity;

    #[test]
    fn keeps_matching_persona_guidance_once() {
        assert_eq!(
            compose_worker_identity(
                Some("Hopper"),
                Some("You are Hopper. Stay focused."),
                Some("Task"),
            )
            .as_deref(),
            Some("You are Hopper. Stay focused.")
        );
    }

    #[test]
    fn composes_persona_with_nonmatching_guidance() {
        assert_eq!(
            compose_worker_identity(
                Some("Shannon"),
                Some("You are a React engineer."),
                Some("Task"),
            )
            .as_deref(),
            Some("You are Shannon. You are a React engineer.")
        );
    }

    #[test]
    fn falls_back_to_executor_line_for_personality_only() {
        assert_eq!(
            compose_worker_identity(Some("Hopper"), None, Some("Wire dispatch RPC")).as_deref(),
            Some("You are Hopper — executor for: Wire dispatch RPC")
        );
    }

    #[test]
    fn preserves_guidance_without_personality_and_omits_absent_legacy_identity() {
        assert_eq!(
            compose_worker_identity(None, Some("Review only."), Some("Task")).as_deref(),
            Some("Review only.")
        );
        assert_eq!(
            compose_worker_identity(None, None, Some("Legacy task")),
            None
        );
        assert_eq!(
            compose_worker_identity(Some("  "), Some("  "), Some("Legacy task")),
            None
        );
    }
}
