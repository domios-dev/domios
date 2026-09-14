//! The review round bound is advice to the Orchestrator, not a gate: the third
//! bounce and the third-round Worker report are both accepted, with no
//! `plan review override` anywhere in either path.

use super::*;

/// Two bounces, then a third. The third re-entry and the third bounce are both
/// accepted with no override anywhere, and the bounce answers with strategy
/// advice for the Orchestrator instead of a refusal.
#[test]
fn third_bounce_is_accepted_with_strategy_advice() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "deep-review", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();

    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    let first = store
        .bounce_review(
            &plan_id,
            &task_id,
            "round 1 findings",
            true,
            false,
            AgentRef::from_tab("rev"),
            5,
        )
        .unwrap();
    assert_eq!(first.review_round, 1);
    assert_eq!(first.strategy_warning, None);
    deliver_bounce(&store, &plan_id, &task_id, &first, 6);

    set_status(&store, &plan_id, &task_id, "in_review", 8).unwrap();
    let second = store
        .bounce_review(
            &plan_id,
            &task_id,
            "round 2 findings",
            true,
            false,
            AgentRef::from_tab("rev"),
            9,
        )
        .unwrap();
    assert_eq!(second.review_round, 2);
    assert_eq!(
        second.strategy_warning, None,
        "the bound is not passed yet, so round two still bounces silently"
    );
    deliver_bounce(&store, &plan_id, &task_id, &second, 10);

    // Previously refused as ReviewRoundExceeded; the bound is advisory now.
    set_status(&store, &plan_id, &task_id, "in_review", 12).unwrap();
    let third = store
        .bounce_review(
            &plan_id,
            &task_id,
            "round 3 findings",
            true,
            false,
            AgentRef::from_tab("rev"),
            13,
        )
        .unwrap();

    assert_eq!(third.verdict, crate::plan::ReviewVerdict::Bounce);
    assert_eq!(third.review_round, 3);
    let advice = third
        .strategy_warning
        .as_ref()
        .expect("the third bounce carries advice to the Orchestrator");
    assert!(
        advice.round_bound_advisory,
        "the advisory must be raised by the round bound, not by a stagnant streak"
    );
    assert_eq!(
        advice.consecutive_bounces, 0,
        "every bounce changed an executable surface, so no stagnant streak is claimed"
    );
    assert_eq!(advice.review_round, 3);
    assert!(!advice.message.trim().is_empty());
    let response = serde_json::to_value(&third).unwrap();
    assert_eq!(
        response["strategy_warning"]["round_bound_advisory"],
        serde_json::json!(true),
        "the CLI response carries the advisory, not just the in-process result"
    );
    assert_eq!(
        response["strategy_warning"]["message"],
        serde_json::json!(advice.message)
    );

    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.status, TaskStatus::InReview);
    assert_eq!(task.review_evidence.len(), 3);
    assert_eq!(
        task.review_override, None,
        "nothing along the way required `plan review override`"
    );
    assert!(task.review_audit.is_empty());
}

/// The other half: the Worker's own third-round report. It is accepted through
/// the ordinary report path, with no override and no refusal text attached to
/// the milestone the Orchestrator reads.
#[test]
fn third_round_report_is_accepted_without_bounce_rejection() {
    let (_dir, store, plan_id) = setup();
    let task_id = add_ready_task(&store, &plan_id, "third-round-report", 2);
    dispatch(&store, &plan_id, &task_id, "tab_impl", 3).unwrap();

    set_status(&store, &plan_id, &task_id, "in_review", 4).unwrap();
    let first = store
        .bounce_review(
            &plan_id,
            &task_id,
            "round 1 findings",
            true,
            false,
            AgentRef::from_tab("rev"),
            5,
        )
        .unwrap();
    deliver_bounce(&store, &plan_id, &task_id, &first, 6);
    set_status(&store, &plan_id, &task_id, "in_review", 8).unwrap();
    let second = store
        .bounce_review(
            &plan_id,
            &task_id,
            "round 2 findings",
            true,
            false,
            AgentRef::from_tab("rev"),
            9,
        )
        .unwrap();
    deliver_bounce(&store, &plan_id, &task_id, &second, 10);

    let before = store.show_task(&task_id).unwrap().task;
    assert_eq!(before.status, TaskStatus::InProgress);
    assert_eq!(before.review_round, 2);
    assert_eq!(before.review_override, None);

    let message = "STATUS: in_review | CHANGES: round three correction".to_string();
    let accepted = store
        .ingest_bound_worker_report(
            &plan_id,
            &task_id,
            &AgentRef::from_tab("tab_impl"),
            crate::service_plan::task_attempts::BoundWorkerReport {
                message: message.clone(),
                status: Some(codeterm_types::agent::ReportStatus::InReview),
                branch: Some("lane/third-round".into()),
                sha: Some("c0ffee".into()),
                changed_files: vec!["src/fix.rs".into()],
                test_cmd: None,
                test_result: None,
                skipped: None,
                authority: None,
            },
            20,
        )
        .expect("a third-round report is not refused by any budget check");
    assert!(accepted, "the report advances the task into review");

    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.status, TaskStatus::InReview);
    assert_eq!(task.review_round, 2);
    assert_eq!(
        task.review_override, None,
        "acceptance required no `plan review override`"
    );
    assert!(task.review_audit.is_empty());
    let report_milestone = task
        .milestones
        .iter()
        .rfind(|milestone| milestone.kind == "worker_report")
        .expect("the report is recorded as the Orchestrator reads it");
    assert!(
        report_milestone.text.ends_with(&message),
        "the milestone must end with the Worker's own words, with no refusal appended: {}",
        report_milestone.text
    );
    let report = task.execution_attempts[0].report.as_ref().unwrap();
    assert_eq!(report.message.as_deref(), Some(message.as_str()));
}
