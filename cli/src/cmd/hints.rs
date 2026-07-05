use serde_json::{json, Value};

const ORCHESTRATION_REMINDER: &str =
    "hint: orchestrating? read `codeterm docs ORCHESTRATION`; before dispatch, run `codeterm plan doctor <plan> --kind quality`";
const ORCHESTRATION_REMINDER_EVERY: u64 = 5;
const ORCHESTRATION_REMINDER_STATE: &str = "orchestration-hint-count";

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

pub fn spec_create_next_plan_hint(spec_id: &str) -> String {
    format!(
        "hint: -> now build the plan: codeterm plan create --from-spec {spec_id} --title <title>"
    )
}

pub fn spec_section_add_link_hint(spec_id: &str) -> String {
    format!(
        "hint: -> link it or build the plan from this spec: codeterm spec link add <plan_id> --spec {spec_id} --spec-section <section_id> --plan-section <section_id> OR codeterm plan create --from-spec {spec_id} --title <title>"
    )
}

pub fn plan_create_spec_first_hint() -> String {
    "WARNING: plans are built from a spec; create one with `codeterm spec create`, or use `plan create --from-spec <id>`".to_string()
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

pub fn orchestration_reminder_for_count(count: u64) -> Option<String> {
    if count == 1 || count.is_multiple_of(ORCHESTRATION_REMINDER_EVERY) {
        Some(ORCHESTRATION_REMINDER.to_string())
    } else {
        None
    }
}

pub fn maybe_orchestration_reminder() -> Option<String> {
    let path = crate::data_root().join(ORCHESTRATION_REMINDER_STATE);
    let current = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let next = current.saturating_add(1);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, next.to_string());
    orchestration_reminder_for_count(next)
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

pub(crate) fn worker_spawn_hints(
    is_worker: bool,
    has_report_target: bool,
    has_task: bool,
) -> Vec<String> {
    let mut hints = Vec::new();
    if !is_worker {
        return hints;
    }
    if !has_report_target {
        hints.push(
            "hint: no --parent/--report-to — this worker's reports land in the spawning pane; pass --parent <orchestrator pane id> so they reach the orchestrator".to_string(),
        );
    }
    if !has_task {
        hints.push(
            "hint: no --task/--task-file — the worker starts idle without a brief; pass the task so it knows what to do".to_string(),
        );
    }
    hints
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_spawn_hints_fire_only_for_workers_missing_bindings() {
        assert!(worker_spawn_hints(false, false, false).is_empty());
        assert!(worker_spawn_hints(true, true, true).is_empty());
        let both = worker_spawn_hints(true, false, false);
        assert_eq!(both.len(), 2);
        assert!(both[0].contains("--parent"));
        assert!(both[1].contains("--task"));
        assert_eq!(worker_spawn_hints(true, true, false).len(), 1);
        assert_eq!(worker_spawn_hints(true, false, true).len(), 1);
    }

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

    #[test]
    fn quality_guard_is_loud_and_actionable() {
        let hint = task_quality_guard_hint("pl_1", "task_1", false, false, false).unwrap();
        assert!(hint.starts_with("WARNING:"));
        assert!(hint.contains("--criterion <condition>"));
        assert!(hint.contains("--test-plan <how verified>"));
        assert!(hint.contains("--correctness <one-line vector>"));
    }

    #[test]
    fn quality_guard_lists_only_missing_fields() {
        let cases = [
            (false, true, true, vec!["acceptance criteria"]),
            (true, false, true, vec!["test plan"]),
            (true, true, false, vec!["correctness vector"]),
        ];
        for (has_criteria, has_test_plan, has_correctness, expected) in cases {
            let hint = task_quality_guard_hint(
                "pl_1",
                "task_1",
                has_criteria,
                has_test_plan,
                has_correctness,
            )
            .unwrap();
            for field in expected {
                assert!(hint.contains(field), "{hint}");
            }
            if has_criteria {
                assert!(!hint.contains("acceptance criteria"), "{hint}");
            }
            if has_test_plan {
                assert!(!hint.contains("task is missing test plan"), "{hint}");
            }
            if has_correctness {
                assert!(!hint.contains("correctness vector"), "{hint}");
            }
        }
        assert!(task_quality_guard_hint("pl_1", "task_1", true, true, true).is_none());
    }

    #[test]
    fn push_hints_omits_empty_and_dedups_existing() {
        let mut value = json!({ "task_id": "task_1", "_hints": ["hint: keep"] });
        push_hints(
            &mut value,
            vec!["".into(), "hint: keep".into(), "hint: add".into()],
        );
        assert_eq!(value["_hints"], json!(["hint: keep", "hint: add"]));
    }

    #[test]
    fn push_hints_does_not_create_field_when_satisfied() {
        let mut value = json!({ "task_id": "task_1" });
        push_hints(&mut value, vec![" ".into()]);
        assert!(value.get("_hints").is_none());
        assert_eq!(value["task_id"], "task_1");
    }

    #[test]
    fn orchestration_reminder_fires_on_first_and_cadence() {
        assert!(orchestration_reminder_for_count(1).is_some());
        assert!(orchestration_reminder_for_count(2).is_none());
        assert!(orchestration_reminder_for_count(5).is_some());
        assert!(orchestration_reminder_for_count(6).is_none());
    }
}
