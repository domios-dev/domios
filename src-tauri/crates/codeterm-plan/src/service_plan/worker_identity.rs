use crate::plan::Task;

pub const WORKER_IDENTITY_END: &str = "<!-- codeterm-worker-identity-end -->";
pub const WORKER_ACK_INSTRUCTION: &str = "Before inspecting files or narrating work, run `codeterm agent ack`. It confirms task ownership and does not replace the final report.";

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

pub fn compose_task_worker_identity(task: &Task, effective_personality: Option<&str>) -> String {
    compose_worker_identity_contract(
        effective_personality,
        task.role_guidance.as_deref(),
        task.executor_role.as_deref(),
        &task.name,
    )
}

pub fn compose_worker_identity_contract(
    personality: Option<&str>,
    role_guidance: Option<&str>,
    executor_role: Option<&str>,
    task_name: &str,
) -> String {
    let mut lines = Vec::new();
    if let Some(identity) = compose_worker_identity(personality, role_guidance, Some(task_name)) {
        lines.push(identity);
    } else {
        lines.push(format!("You are the Worker responsible for: {task_name}"));
    }
    if let Some(role) = trimmed(executor_role) {
        lines.push(format!("Executor role: {role}."));
    }
    if !lines[0].contains(task_name) {
        lines.push(format!("Your assigned task is: {task_name}"));
    }
    lines.join("\n")
}

pub fn attach_worker_identity(identity: &str, assignment: &str) -> String {
    format!(
        "{}\n\n{}\n\n{}",
        identity.trim(),
        WORKER_IDENTITY_END,
        assignment.trim_start()
    )
}

pub fn split_worker_identity(prompt: &str) -> (Option<&str>, &str) {
    let Some((identity, assignment)) = prompt.split_once(WORKER_IDENTITY_END) else {
        return (None, prompt);
    };
    let identity = identity.trim();
    (
        (!identity.is_empty()).then_some(identity),
        assignment.trim_start(),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        attach_worker_identity, compose_worker_identity, compose_worker_identity_contract,
        split_worker_identity,
    };

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

    #[test]
    fn task_identity_leads_with_persona_role_and_purpose() {
        let identity = compose_worker_identity_contract(
            Some("Hopper"),
            Some("Audit the evidence boundary."),
            Some("rust-backend-dev"),
            "Repair delivery",
        );

        assert_eq!(
            identity,
            "You are Hopper. Audit the evidence boundary.\n\
             Executor role: rust-backend-dev.\n\
             Your assigned task is: Repair delivery"
        );
    }

    #[test]
    fn identity_transport_marker_is_removed_by_split() {
        let prompt = attach_worker_identity("You are Hopper.", "# Repair delivery");
        let (identity, assignment) = split_worker_identity(&prompt);

        assert_eq!(identity, Some("You are Hopper."));
        assert_eq!(assignment, "# Repair delivery");
    }
}
