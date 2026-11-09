use super::*;

#[test]
fn task_create_hints_include_section_and_model_fixes() {
    let hints = task_create_hints("pl_1", "task_1", false, false, true, true, true, true);
    assert_eq!(hints.len(), 2);
    assert!(hints[0].contains("domios plan task link pl_1 task_1 --sections <id-or-handle>"));
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

#[test]
fn task_create_hints_fire_only_for_missing_fields() {
    struct Case {
        name: &'static str,
        has_sections: bool,
        has_model: bool,
        has_why: bool,
        has_criteria: bool,
        has_test_plan: bool,
        has_correctness: bool,
        expected: Vec<&'static str>,
    }

    let cases = [
        Case {
            name: "missing section",
            has_sections: false,
            has_model: true,
            has_why: true,
            has_criteria: true,
            has_test_plan: true,
            has_correctness: true,
            expected: vec!["plan task link"],
        },
        Case {
            name: "missing model",
            has_sections: true,
            has_model: false,
            has_why: true,
            has_criteria: true,
            has_test_plan: true,
            has_correctness: true,
            expected: vec!["--model <provider/model> --why <reason>"],
        },
        Case {
            name: "missing why",
            has_sections: true,
            has_model: true,
            has_why: false,
            has_criteria: true,
            has_test_plan: true,
            has_correctness: true,
            expected: vec!["--model <provider/model> --why <reason>"],
        },
        Case {
            name: "missing criteria",
            has_sections: true,
            has_model: true,
            has_why: true,
            has_criteria: false,
            has_test_plan: true,
            has_correctness: true,
            expected: vec!["acceptance criteria"],
        },
        Case {
            name: "missing test plan",
            has_sections: true,
            has_model: true,
            has_why: true,
            has_criteria: true,
            has_test_plan: false,
            has_correctness: true,
            expected: vec!["task is missing test plan"],
        },
        Case {
            name: "missing correctness",
            has_sections: true,
            has_model: true,
            has_why: true,
            has_criteria: true,
            has_test_plan: true,
            has_correctness: false,
            expected: vec!["correctness vector"],
        },
        Case {
            name: "satisfied",
            has_sections: true,
            has_model: true,
            has_why: true,
            has_criteria: true,
            has_test_plan: true,
            has_correctness: true,
            expected: vec![],
        },
    ];

    for case in cases {
        let hints = task_create_hints(
            "pl_1",
            "task_1",
            case.has_sections,
            case.has_model,
            case.has_why,
            case.has_criteria,
            case.has_test_plan,
            case.has_correctness,
        );
        assert_eq!(hints.len(), case.expected.len(), "{}", case.name);
        let joined = hints.join("\n");
        for expected in case.expected {
            assert!(joined.contains(expected), "{}: {joined}", case.name);
        }
    }
}

#[test]
fn orchestration_reminder_fires_on_first_and_cadence() {
    assert!(orchestration_reminder_for_count(1).is_some());
    assert!(orchestration_reminder_for_count(2).is_none());
    assert!(orchestration_reminder_for_count(5).is_some());
    assert!(orchestration_reminder_for_count(6).is_none());
}

fn make_task(
    id: &str,
    kind: &str,
    status: TaskStatus,
    handle: Option<&str>,
    tab: Option<&str>,
) -> crate::plan::Task {
    crate::plan::Task {
        files: Default::default(),
        id: id.into(),
        name: id.into(),
        kind: kind.into(),
        model: None,
        speed: None,
        model_rationale: None,
        acceptance_criteria: vec![],
        external_refs: vec![],
        test_plan: None,
        qa_plan: None,
        correctness: None,
        status,
        review_stage: Default::default(),
        qa_state: None,
        review_round: 0,
        review_oracle: None,
        review_evidence: vec![],
        key: None,
        review_override: None,
        review_audit: vec![],
        status_label: None,
        priority: crate::plan::TaskPriority::P2,
        created_at: 0,
        updated_at: 0,
        status_changed_at: 0,
        handle: handle.map(str::to_string),
        tags: vec![],
        section_ids: vec![],
        body: String::new(),
        role_guidance: None,
        executor_role: None,
        competition: None,
        intent: None,
        execution_intent: Default::default(),
        parent_task_id: None,
        depends_on: vec![],
        assigned_tab_id: tab.map(str::to_string),
        lane: crate::plan::TaskLaneMetadata::default(),
        taken_by_role: None,
        peer: None,
        group: None,
        canvas: crate::plan::ElementCanvas::default(),
        milestones: vec![],
        execution_attempts: vec![],
        interruption: Default::default(),
        pause_reason: None,
        personality: None,
        personality_provenance: None,
        attachments: vec![],
    }
}

fn make_plan(id: &str, tasks: Vec<crate::plan::Task>) -> Plan {
    let mut plan = Plan::new("T".into(), "/tmp/p".into(), Default::default());
    plan.id = id.into();
    plan.tasks = tasks;
    plan
}

#[test]
fn nudge_fires_when_planner_assigned_to_different_tab() {
    let plan = make_plan(
        "pl_1",
        vec![make_task(
            "task_planner",
            "planning",
            TaskStatus::InProgress,
            Some("plan-wave"),
            Some("tab-planner"),
        )],
    );
    let hint = planner_ownership_nudge(&plan, Some("tab-orchestrator"));
    assert!(hint.is_some(), "should emit hint for non-planner caller");
    let hint = hint.unwrap();
    assert!(hint.contains("pl_1"), "should name the plan");
    assert!(hint.contains("plan-wave"), "should use the task handle");
    assert!(
        hint.contains("route planning input"),
        "should be actionable"
    );
}

#[test]
fn nudge_silent_when_caller_is_the_planner_tab() {
    let plan = make_plan(
        "pl_1",
        vec![make_task(
            "task_planner",
            "planning",
            TaskStatus::InProgress,
            None,
            Some("tab-planner"),
        )],
    );
    assert!(
        planner_ownership_nudge(&plan, Some("tab-planner")).is_none(),
        "planner calling its own mutations should not see the hint"
    );
}

#[test]
fn nudge_silent_when_planning_task_done_or_cancelled() {
    for status in [TaskStatus::Done, TaskStatus::Cancelled] {
        let plan = make_plan(
            "pl_1",
            vec![make_task("t", "planning", status, None, Some("tab-x"))],
        );
        assert!(
            planner_ownership_nudge(&plan, Some("tab-other")).is_none(),
            "closed planning tasks must not trigger the hint"
        );
    }
}

#[test]
fn nudge_silent_when_no_planning_task_has_assigned_tab() {
    let plan = make_plan(
        "pl_1",
        vec![make_task(
            "t",
            "planning",
            TaskStatus::Pending,
            None,
            None, // unassigned
        )],
    );
    assert!(
        planner_ownership_nudge(&plan, Some("tab-other")).is_none(),
        "unassigned planning task provides no routing target"
    );
}

#[test]
fn nudge_uses_task_name_when_handle_is_absent() {
    let plan = make_plan(
        "pl_1",
        vec![make_task(
            "planning-task-name",
            "planning",
            TaskStatus::InProgress,
            None,
            Some("tab-planner"),
        )],
    );
    let hint = planner_ownership_nudge(&plan, None).unwrap();
    assert!(hint.contains("planning-task-name"));
}
