use super::*;
use serde_json::json;

fn plan_value() -> Value {
    json!({
        "id": "pl_rows",
        "title": "Rows",
        "sections": [{"id": "s1", "title": "Section", "body": "text"}],
        "tasks": [
            {
                "id": "t2",
                "name": "Second listed first",
                "updated_at": 9,
                "milestones": [{"id": "m1", "text": "one"}, {"id": "m2", "text": "two"}],
                "execution_attempts": [{"id": "a2", "state": "running"}, {"id": "a1", "state": "done"}]
            },
            {"id": "t1", "name": "No history", "pane_era_field": {"pane_id": "kept verbatim"}}
        ]
    })
}

fn joined(rows: &PlanRows) -> Value {
    let tasks = rows
        .tasks
        .iter()
        .map(|task| StoredTask {
            document: serde_json::from_str(&task.document).unwrap(),
            milestones: task
                .milestones
                .iter()
                .map(|milestone| serde_json::from_str(milestone).unwrap())
                .collect(),
            attempts: task
                .attempts
                .iter()
                .map(|attempt| serde_json::from_str(&attempt.document).unwrap())
                .collect(),
        })
        .collect();
    join_plan(serde_json::from_str(&rows.plan).unwrap(), tasks).unwrap()
}

#[test]
fn split_then_join_restores_every_field_and_order() {
    let rows = split_plan(plan_value()).unwrap();

    assert!(!rows.plan.contains("\"tasks\""));
    assert_eq!(
        rows.tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        ["t2", "t1"]
    );
    assert!(!rows.tasks[0].document.contains("milestones"));
    assert_eq!(
        rows.tasks[0]
            .attempts
            .iter()
            .map(|attempt| attempt.id.as_str())
            .collect::<Vec<_>>(),
        ["a2", "a1"]
    );
    let mut expected = plan_value();
    expected["tasks"][1]["milestones"] = json!([]);
    expected["tasks"][1]["execution_attempts"] = json!([]);
    assert_eq!(joined(&rows), expected);
}

#[test]
fn milestone_digests_chain_so_an_edited_prefix_is_detectable() {
    let original = split_plan(plan_value()).unwrap();
    let mut appended_value = plan_value();
    appended_value["tasks"][0]["milestones"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "m3", "text": "three"}));
    let appended = split_plan(appended_value).unwrap();
    let mut edited_value = plan_value();
    edited_value["tasks"][0]["milestones"][0]["text"] = json!("edited");
    let edited = split_plan(edited_value).unwrap();

    assert_eq!(
        appended.tasks[0].milestone_digests[1],
        original.tasks[0].milestone_digest()
    );
    assert_ne!(
        edited.tasks[0].milestone_digests[1],
        original.tasks[0].milestone_digest()
    );
    assert_eq!(appended.tasks[0].hash, original.tasks[0].hash);
}

#[test]
fn a_plan_row_that_embeds_tasks_is_refused() {
    let error = join_plan(plan_value(), Vec::new()).unwrap_err();
    assert!(error.contains("unsupported whole-document plan row"));
}

#[test]
fn unkeyable_tasks_are_refused() {
    for unkeyable in [json!("t2"), json!(7)] {
        let mut value = plan_value();
        value["tasks"][1]["id"] = unkeyable;
        let error = split_plan(value).unwrap_err();
        assert!(error.contains("duplicate task id") || error.contains("no string id"));
    }
}

#[test]
fn duplicate_or_missing_attempt_ids_keep_their_positions() {
    let mut duplicate_attempt = plan_value();
    duplicate_attempt["tasks"][0]["execution_attempts"][1]["id"] = json!("a2");
    duplicate_attempt["tasks"][0]["execution_attempts"]
        .as_array_mut()
        .unwrap()
        .push(json!({"state": "no id"}));
    let rows = split_plan(duplicate_attempt.clone()).unwrap();
    assert_eq!(rows.tasks[0].attempts.len(), 3);
    assert_eq!(rows.tasks[0].attempts[2].id, "");
    duplicate_attempt["tasks"][1]["milestones"] = json!([]);
    duplicate_attempt["tasks"][1]["execution_attempts"] = json!([]);
    assert_eq!(joined(&rows), duplicate_attempt);
}

#[test]
fn an_oversized_row_is_refused_while_a_large_plan_is_not() {
    let mut value = plan_value();
    let milestones = (0..6)
        .map(|seq| json!({"id": format!("big{seq}"), "text": "x".repeat(3 * 1024 * 1024)}))
        .collect::<Vec<_>>();
    value["tasks"][1]["milestones"] = Value::Array(milestones);
    let rows = split_plan(value.clone()).unwrap();
    validate_row_sizes(&rows).unwrap();

    value["tasks"][1]["body"] = json!("y".repeat(MAX_STORED_ROW_BYTES));
    let rows = split_plan(value).unwrap();
    assert!(validate_row_sizes(&rows).unwrap_err().contains("task row"));
}
