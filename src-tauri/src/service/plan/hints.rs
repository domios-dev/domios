pub fn task_sections_hint(plan_id: &str, task_id: &str) -> String {
    format!(
        "hint: task is not linked to a plan section. If it belongs to a plan, connect it: codeterm plan task update {plan_id} {task_id} --sections <id> (ids: codeterm plan get {plan_id} --sections)"
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

pub fn task_create_hints(
    plan_id: &str,
    task_id: &str,
    has_sections: bool,
    has_model: bool,
    has_why: bool,
) -> Vec<String> {
    let mut hints = vec![];
    if !has_sections {
        hints.push(task_sections_hint(plan_id, task_id));
    }
    if !has_model || !has_why {
        hints.push(task_model_hint(plan_id, task_id));
    }
    hints
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_create_hints_include_section_and_model_fixes() {
        let hints = task_create_hints("pl_1", "task_1", false, false, true);
        assert_eq!(hints.len(), 2);
        assert!(hints[0].contains("codeterm plan task update pl_1 task_1 --sections <id>"));
        assert!(hints[1].contains("--model <provider/model> --why <reason>"));
    }
}
