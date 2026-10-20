use super::*;

fn plan() -> Plan {
    let mut plan = Plan::new("title-token".into(), "/repo".into(), Default::default());
    plan.id = "plan-token".into();
    plan.team_id = Some("team-token".into());
    plan
}

#[test]
fn role_substitution_and_fragment_transport() {
    let plan = plan();
    let expected = ROLE_HEADER
        .replace("{team}", "team-token")
        .replace("{plan}", "plan-token")
        .replace("{title}", "title-token");
    assert_eq!(
        compose(&plan, &["opaque-a", "opaque-b"]),
        format!("{expected}\nopaque-a\nopaque-b")
    );
    assert!(!role_header(&plan).contains('{'));
}

#[test]
fn task_labels_and_unicode_bound() {
    let mut plan = plan();
    let task: Task = serde_json::from_value(serde_json::json!({
        "id": "task-token", "key": "KEY-42", "name": "name-token", "type": "implement",
        "status": "pending"
    }))
    .unwrap();
    assert_eq!(task_label(&task), "KEY-42 name-token");
    plan.tasks.push(task);
    assert_eq!(
        label_task_references(&plan, "(task-token), task-token-extra"),
        "(KEY-42 name-token), task-token-extra"
    );
    assert_eq!(
        compose(&plan, &["task-token"]),
        format!("{}\nKEY-42 name-token", role_header(&plan))
    );
    let oversized = "界".repeat(MAX_REMINDER_BYTES);
    let text = compose(&plan, &["opaque-first", &oversized]);
    assert!(text.len() <= MAX_REMINDER_BYTES);
    assert_eq!(
        text,
        format!("{}\nopaque-first{MORE}plan-token", role_header(&plan))
    );
}

#[test]
fn groups_show_short_labels_and_count_overflow() {
    let mut plan = plan();
    for index in 0..7 {
        plan.tasks.push(
            serde_json::from_value(serde_json::json!({
                "id": format!("opaque-{index}"), "key": format!("KEY-{index}"), "name": "task-name",
                "type": "implement", "status": "pending"
            }))
            .unwrap(),
        );
    }
    let expected = (0..5)
        .map(|i| format!("KEY-{i} task-name"))
        .collect::<Vec<_>>()
        .join(", ");
    assert_eq!(
        action_groups(&plan),
        format!("{DISPATCH}: {expected} (+2 more)")
    );
    plan.tasks[0].status = crate::plan::TaskStatus::InReview;
    plan.tasks[1].status = crate::plan::TaskStatus::Blocked;
    let groups = action_groups(&plan);
    assert!(groups
        .lines()
        .any(|line| line == format!("{REVIEW}: KEY-0 task-name")));
    assert!(groups
        .lines()
        .any(|line| line == format!("{INCIDENTS}: KEY-1 task-name")));
}
