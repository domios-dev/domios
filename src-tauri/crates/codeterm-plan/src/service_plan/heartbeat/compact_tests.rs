use super::*;
use crate::plan::{QaState, ReviewStage, Task};

fn plan() -> Plan {
    let mut plan = Plan::new("fixture".into(), String::new(), Default::default());
    plan.id = "fixture".into();
    plan.mode = PlanMode::Execution;
    plan.planning_state = crate::plan::PlanningState::Approved;
    plan
}

fn task(id: &str, status: TaskStatus) -> Task {
    serde_json::from_value(serde_json::json!({
        "id": id, "name": id, "type": "implement", "status": status
    }))
    .unwrap()
}

#[test]
fn recurring_fixture_is_at_least_three_times_smaller_and_preserves_owner_rules() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../evals/orchestrator-control-loop/fixtures/reminder.json"
    )))
    .unwrap();
    let mut plan = plan();
    plan.id = fixture["plan_id"].as_str().unwrap().into();
    plan.heartbeat.learnings = serde_json::from_value(fixture["learnings"].clone()).unwrap();
    let payload = fixture["payload"].as_str().unwrap();
    let text = prepend_plan_heartbeat(&plan, payload);
    let before = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../evals/orchestrator-control-loop/fixtures/before-reminder.md"
    ))
    .len();
    println!(
        "reminder fixture bytes before={before} after={}",
        text.len()
    );
    assert!(text.len() * 3 <= before);
    assert!(text.len() <= 1500);
    assert!(text.ends_with(payload));
    assert!(!text.contains(STATE_RECONCILIATION_PROMPT));
    assert!(text.contains(HEARTBEAT_MORE));
    for learning in plan
        .heartbeat
        .learnings
        .iter()
        .filter(|learning| is_owner_rule(learning))
    {
        assert_eq!(text.matches(learning.text.as_str()).count(), 1);
    }
    let situational = plan
        .heartbeat
        .learnings
        .iter()
        .filter(|learning| !is_owner_rule(learning))
        .filter(|learning| text.contains(&learning.id))
        .count();
    assert_eq!(situational, MAX_SITUATIONAL_LEARNINGS);
}

#[test]
fn owner_rules_survive_empty_and_non_owner_supporter_selection() {
    let (_dir, store, plan_id) = super::super::heartbeat_tests::store_with_plan("owner");
    let owner_fragments = (0..6)
        .map(|index| format!("opaque-owner-rule-{index}"))
        .collect::<Vec<_>>();
    let mut owner_ids = Vec::new();
    for (index, fragment) in owner_fragments.iter().enumerate() {
        owner_ids.push(
            store
                .add_heartbeat_learning(
                    Some(&plan_id),
                    None,
                    None,
                    fragment,
                    Some("owner"),
                    index as u64 + 20,
                )
                .unwrap()
                .learning
                .id,
        );
    }
    let fragments = (0..5)
        .map(|index| format!("opaque-situation-{index}"))
        .collect::<Vec<_>>();
    let mut ids = owner_ids;
    for (index, fragment) in fragments.iter().enumerate() {
        ids.push(
            store
                .add_heartbeat_learning(
                    Some(&plan_id),
                    None,
                    None,
                    fragment,
                    None,
                    index as u64 + 40,
                )
                .unwrap()
                .learning
                .id,
        );
    }
    for selection in [Some(&ids[..]), Some(&[][..]), None] {
        let rendered =
            store.prepend_heartbeat_with_selected_learnings(&plan_id, "", &[], selection);
        for fragment in &owner_fragments {
            assert_eq!(rendered.matches(fragment.as_str()).count(), 1);
        }
        for (index, fragment) in fragments.iter().enumerate() {
            let included = match selection {
                Some([]) => false,
                Some(_) => index < 3,
                None => index >= 2,
            };
            assert_eq!(rendered.contains(fragment), included);
        }
    }
    assert_eq!(store.get(&plan_id).unwrap().heartbeat.learnings.len(), 11);
}

#[test]
fn next_action_projection_names_only_work_that_can_advance() {
    let mut plan = plan();
    let mut waiting = task("opaque-wait", TaskStatus::Pending);
    waiting.depends_on = vec!["opaque-running".into()];
    let mut missing = task("opaque-missing", TaskStatus::Pending);
    missing.depends_on = vec!["missing-prerequisite".into()];
    let mut held = task("opaque-held", TaskStatus::InReview);
    held.review_evidence.push(crate::plan::ReviewEvidence {
        id: "hold".into(),
        from: Default::default(),
        verdict: crate::plan::ReviewVerdict::Hold,
        note: "opaque hold".into(),
        at: 1,
        changed_executable_surfaces: false,
        distinct_oracle: false,
    });
    let mut queued = task("opaque-qa", TaskStatus::InReview);
    queued.review_stage = ReviewStage::Qa;
    queued.qa_state = Some(QaState::Queued);
    let mut qa_running = queued.clone();
    qa_running.id = "opaque-qa-running".into();
    qa_running.qa_state = Some(QaState::Running);
    plan.tasks = vec![
        task("opaque-pending", TaskStatus::Pending),
        task("opaque-review", TaskStatus::InReview),
        task("opaque-blocked", TaskStatus::Blocked),
        task("opaque-running", TaskStatus::InProgress),
        task("opaque-paused", TaskStatus::Paused),
        waiting,
        missing,
        held,
        queued,
        qa_running,
    ];
    let snapshot = reminder_snapshot(&plan);
    let mut rendered = String::new();
    let named = render_actions(&plan, &snapshot, &mut rendered);
    for id in [
        "opaque-pending",
        "opaque-review",
        "opaque-blocked",
        "opaque-missing",
        "opaque-qa",
    ] {
        assert!(named.iter().any(|named| named == id));
        assert_eq!(rendered.matches(id).count(), 1);
    }
    for id in [
        "opaque-running",
        "opaque-paused",
        "opaque-wait",
        "opaque-held",
        "opaque-qa-running",
    ] {
        assert!(!named.iter().any(|named| named == id));
        assert!(!rendered.contains(id));
    }
}

#[test]
fn excerpts_keep_unicode_boundaries_and_point_to_full_learnings() {
    let learning = HeartbeatLearning {
        id: "opaque-id".into(),
        text: "🦀".repeat(200),
        ts: 1,
        tag: None,
    };
    let mut rendered = String::new();
    render_situational_learning(&learning, &mut rendered);
    assert!(rendered.contains(&learning.id));
    assert!(!rendered.contains(&learning.text));
    assert!(rendered.len() < learning.text.len());
    let mut plan = plan();
    plan.heartbeat.learnings.push(learning);
    assert!(prepend_plan_heartbeat(&plan, "").contains(HEARTBEAT_MORE));
}

#[test]
fn group_lines_name_only_terminal_groups_with_outstanding_records() {
    let mut plan = plan();
    let mut member = task("opaque-member", TaskStatus::Pending);
    let group = "opaque-group";
    member.group = Some(group.into());
    member.status_changed_at = 10;
    plan.tasks.push(member);
    let mut rendered = String::new();
    render_regression_gates(&plan, &mut rendered);
    render_group_retrospectives(&plan, &mut rendered);
    assert!(rendered.is_empty());
    plan.tasks[0].status = TaskStatus::Done;
    render_regression_gates(&plan, &mut rendered);
    render_group_retrospectives(&plan, &mut rendered);
    assert_eq!(rendered.matches(group).count(), 2);
    for kind in [
        super::super::prompts_plan::REGRESSION_MILESTONE_KIND,
        super::super::reflection::GROUP_CLOSE_MILESTONE_KIND,
    ] {
        plan.strides.push(crate::plan::Milestone {
            id: kind.into(),
            from: Default::default(),
            kind: kind.into(),
            text: String::new(),
            ts: 11,
            links: vec![super::super::milestone_gates::regression_link(group)],
            loop_id: None,
            iteration: None,
            canvas: Default::default(),
            attachments: vec![],
        });
    }
    rendered.clear();
    render_regression_gates(&plan, &mut rendered);
    render_group_retrospectives(&plan, &mut rendered);
    assert!(rendered.is_empty());
}
