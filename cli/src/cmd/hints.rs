use serde_json::{json, Value};

pub fn task_sections_hint(plan_id: &str, task_id: &str) -> String {
    format!(
        "hint: task is not linked to a plan section. If it belongs to a plan, connect it: codeterm plan task link {plan_id} {task_id} --sections <id-or-handle> (ids: codeterm plan get {plan_id} --sections)"
    )
}

pub fn task_model_hint(plan_id: &str, task_id: &str) -> String {
    format!(
        "hint: task has no model/why. Capture routing intent: codeterm plan task update {plan_id} {task_id} --model <provider/model> --why <reason>"
    )
}

pub fn push_hints(value: &mut Value, hints: Vec<String>) {
    let hints: Vec<String> = hints
        .into_iter()
        .filter(|hint| !hint.trim().is_empty())
        .collect();
    if hints.is_empty() {
        return;
    }
    match value {
        Value::Object(obj) => {
            let existing = obj
                .get("_hints")
                .and_then(|v| v.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let mut merged = existing;
            for hint in hints {
                if !merged.iter().any(|item| item == &hint) {
                    merged.push(hint);
                }
            }
            obj.insert("_hints".into(), json!(merged));
        }
        _ => {
            *value = json!({ "value": value.take(), "_hints": hints });
        }
    }
}

pub fn print_hints(hints: &[String]) {
    for hint in hints {
        println!("{hint}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_hint_points_to_update_and_section_ids() {
        let hint = task_sections_hint("pl_1", "task_1");
        assert!(hint.contains("codeterm plan task link pl_1 task_1 --sections <id-or-handle>"));
        assert!(hint.contains("codeterm plan get pl_1 --sections"));
    }

    #[test]
    fn json_hints_are_separate_field() {
        let mut value = json!({ "task_id": "task_1" });
        push_hints(&mut value, vec!["hint: x".into()]);
        assert_eq!(value["_hints"][0], "hint: x");
        assert_eq!(value["task_id"], "task_1");
    }
}
