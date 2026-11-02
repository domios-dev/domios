use crate::plan::Task;

pub use codeterm_types::plan::{task_executor_name, validate_executor_name};

pub const WORKER_IDENTITY_END: &str = "<!-- codeterm-worker-identity-end -->";
pub const WORKER_ACK_INSTRUCTION: &str = "Highest-priority startup action: run domios agent ack before any work. If it is refused or fails, send the exact error to your parent once and continue the assigned work; never retry it in a loop.";

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
        "{}\n\n{}\n\n{}\n\n{}",
        identity.trim(),
        WORKER_IDENTITY_END,
        WORKER_ACK_INSTRUCTION,
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
        split_worker_identity, WORKER_IDENTITY_END,
    };

    #[test]
    fn keeps_matching_persona_guidance_once() {
        let persona = "opaque-persona-81";
        let guidance = format!("You are {persona} opaque-guidance-81");
        let composed = compose_worker_identity(Some(persona), Some(&guidance), None).unwrap();
        assert_eq!(composed, guidance);
        assert_eq!(composed.matches(persona).count(), 1);
    }

    #[test]
    fn composes_persona_with_nonmatching_guidance() {
        let composed = compose_worker_identity(
            Some("opaque-persona-82"),
            Some("opaque-guidance-82"),
            Some("opaque-task-82"),
        )
        .unwrap();
        assert!(composed.contains("opaque-persona-82"));
        assert!(composed.contains("opaque-guidance-82"));
        assert!(composed.find("opaque-persona-82") < composed.find("opaque-guidance-82"));
    }

    #[test]
    fn falls_back_to_executor_line_for_personality_only() {
        let composed =
            compose_worker_identity(Some("opaque-persona-83"), None, Some("opaque-task-83"))
                .unwrap();
        assert!(composed.contains("opaque-persona-83"));
        assert!(composed.contains("opaque-task-83"));
    }

    #[test]
    fn preserves_guidance_without_personality_and_omits_absent_legacy_identity() {
        assert_eq!(
            compose_worker_identity(None, Some("opaque-guidance-84"), Some("opaque-task-84"))
                .as_deref(),
            Some("opaque-guidance-84")
        );
        assert_eq!(
            compose_worker_identity(None, None, Some("opaque-task-84")),
            None
        );
        assert_eq!(
            compose_worker_identity(Some("  "), Some("  "), Some("opaque-task-84")),
            None
        );
    }

    #[test]
    fn task_identity_leads_with_persona_role_and_purpose() {
        let identity = compose_worker_identity_contract(
            Some("opaque-persona-85"),
            Some("opaque-guidance-85"),
            Some("opaque-role-85"),
            "opaque-task-85",
        );
        assert!(identity.contains("opaque-persona-85"));
        assert!(identity.contains("opaque-guidance-85"));
        assert!(identity.contains("opaque-role-85"));
        assert!(identity.contains("opaque-task-85"));
    }

    #[test]
    fn identity_transport_marker_is_removed_by_split() {
        let prompt = attach_worker_identity("opaque-identity-86", "opaque-assignment-86");
        let (identity, assignment) = split_worker_identity(&prompt);

        assert_eq!(identity, Some("opaque-identity-86"));
        assert!(assignment.contains("opaque-assignment-86"));
        assert!(!assignment.contains(WORKER_IDENTITY_END));
    }
}
