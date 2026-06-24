pub fn task_sections_hint(plan_id: &str, task_id: &str) -> String {
    format!(
        "hint: task is not linked to a plan section. If it belongs to a plan, connect it: codeterm plan task link {plan_id} {task_id} --sections <id-or-handle> (ids: codeterm plan get {plan_id} --sections)"
    )
}

pub fn plan_sections_hint(plan_id: &str) -> String {
    format!("hint: plan has no sections. Add one: codeterm plan section add {plan_id} --title <title> --body <summary>")
}

pub fn task_model_hint(plan_id: &str, task_id: &str) -> String {
    format!(
        "hint: task has no model/why. Capture routing intent: codeterm plan task update {plan_id} {task_id} --model <provider/model> --why <reason>"
    )
}

pub fn task_quality_guard_hint(
    plan_id: &str,
    task_id: &str,
    has_criteria: bool,
    has_test_plan: bool,
    has_correctness: bool,
) -> Option<String> {
    let mut missing = vec![];
    if !has_criteria {
        missing.push("acceptance criteria");
    }
    if !has_test_plan {
        missing.push("test plan");
    }
    if !has_correctness {
        missing.push("correctness vector");
    }
    if missing.is_empty() {
        return None;
    }
    Some(format!(
        "WARNING: task is missing {}. Add checkable work definition now: codeterm plan task add {plan_id} --name <name> --criterion <condition> --test-plan <how verified> --correctness <one-line vector>; existing task: {task_id}",
        missing.join(", ")
    ))
}

pub fn task_create_hints(
    plan_id: &str,
    task_id: &str,
    has_sections: bool,
    has_model: bool,
    has_why: bool,
    has_criteria: bool,
    has_test_plan: bool,
    has_correctness: bool,
) -> Vec<String> {
    let mut hints = vec![];
    if !has_sections {
        hints.push(task_sections_hint(plan_id, task_id));
    }
    if !has_model || !has_why {
        hints.push(task_model_hint(plan_id, task_id));
    }
    if let Some(hint) = task_quality_guard_hint(
        plan_id,
        task_id,
        has_criteria,
        has_test_plan,
        has_correctness,
    ) {
        hints.push(hint);
    }
    hints
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_create_hints_include_section_and_model_fixes() {
        let hints = task_create_hints("pl_1", "task_1", false, false, true, true, true, true);
        assert_eq!(hints.len(), 2);
        assert!(hints[0].contains("codeterm plan task link pl_1 task_1 --sections <id-or-handle>"));
        assert!(hints[1].contains("--model <provider/model> --why <reason>"));
    }

    #[test]
    fn task_create_hints_warn_loudly_without_quality_fields() {
        let hints = task_create_hints("pl_1", "task_1", true, true, true, false, false, false);
        assert_eq!(hints.len(), 1);
        assert!(hints[0].starts_with("WARNING:"));
        assert!(hints[0].contains("acceptance criteria"));
        assert!(hints[0].contains("--criterion <condition>"));
        assert!(hints[0].contains("--test-plan <how verified>"));
        assert!(hints[0].contains("--correctness <one-line vector>"));
    }
}
