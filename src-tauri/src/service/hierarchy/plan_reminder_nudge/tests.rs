use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::plan::{ElementCanvas, OrchestratorRef, Task, TaskLaneMetadata, TaskPriority};
use codeterm_types::pty::BootStage;
use std::cell::RefCell;

fn reminder_test_tab() -> (MultiplexerState, String) {
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.set_orchestrator_role(&tab_id).unwrap();
    (mux, tab_id)
}

fn plan(id: &str, owner: &str, mode: PlanMode, tasks: Vec<Task>) -> Plan {
    let mut plan = Plan::new(
        "Reminder".into(),
        "/tmp/project".into(),
        OrchestratorRef::from_tab(owner),
    );
    plan.id = id.into();
    plan.mode = mode;
    plan.tasks = tasks;
    plan
}

fn task(id: &str, status: TaskStatus, tab_id: Option<&str>) -> Task {
    Task {
        id: id.into(),
        name: id.into(),
        kind: "status_update".into(),
        model: None,
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
        priority: TaskPriority::P2,
        created_at: 0,
        updated_at: 0,
        status_changed_at: 0,
        handle: None,
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
        assigned_tab_id: tab_id.map(str::to_string),
        lane: TaskLaneMetadata::default(),
        taken_by_role: None,
        peer: None,
        group: None,
        canvas: ElementCanvas::default(),
        milestones: vec![],
        execution_attempts: vec![],
        pause_reason: None,
        interruption: Default::default(),
        personality: None,
        personality_provenance: None,
        attachments: vec![],
    }
}

fn aged_item(t: &Task, age_ms: u64) -> crate::plan::reconcile::AgedOpenTask {
    let projection_plan = plan("projection", "orch", PlanMode::Execution, vec![t.clone()]);
    let projection = task_projection(&projection_plan, &projection_plan.tasks[0]);
    crate::plan::reconcile::AgedOpenTask {
        task_id: t.id.clone(),
        name: t.name.clone(),
        status: t.status.clone(),
        presentation: projection.presentation,
        dependency_waits: projection.dependency_waits,
        age_ms,
    }
}

#[test]
fn queued_qa_task_is_not_reported_unjudged() {
    let mut t = task("t_queued", TaskStatus::InReview, None);
    t.review_stage = crate::plan::ReviewStage::Qa;
    t.qa_state = Some(crate::plan::QaState::Queued);
    t.status_label = Some("blocked: reviewer host unavailable overnight".into());
    let item = aged_item(&t, 3 * 3_600_000);

    let text = aged_task_status_text(&item, Some(&t));

    assert_eq!(
        text,
        "queued for QA: blocked: reviewer host unavailable overnight"
    );
    assert!(
        !text.contains("InReview"),
        "must not read as generically unjudged in_review: {text}"
    );
}

#[test]
fn queued_qa_task_without_label_degrades_honestly() {
    let mut t = task("t_queued", TaskStatus::InReview, None);
    t.review_stage = crate::plan::ReviewStage::Qa;
    t.qa_state = Some(crate::plan::QaState::Queued);
    let item = aged_item(&t, 3 * 3_600_000);

    assert_eq!(aged_task_status_text(&item, Some(&t)), "queued for QA");
}

#[test]
fn plain_aged_in_review_task_keeps_its_status_text() {
    let t = task("t_plain", TaskStatus::InReview, None);
    let item = aged_item(&t, 3 * 3_600_000);

    assert_eq!(aged_task_status_text(&item, Some(&t)), "in review");
}

#[test]
fn running_qa_task_is_not_presented_as_queued() {
    let mut t = task("t_running", TaskStatus::InReview, None);
    t.review_stage = crate::plan::ReviewStage::Qa;
    t.qa_state = Some(crate::plan::QaState::Running);
    t.status_label = Some("irrelevant while running".into());
    let item = aged_item(&t, 3 * 3_600_000);

    assert_eq!(aged_task_status_text(&item, Some(&t)), "in review");
}

const AGED_NOW_MS: u64 = crate::plan::reconcile::PENDING_UNSTAFFED_STALE_MS + 10;

fn dependency_fixture() -> Plan {
    let mut active = task("dep_active", TaskStatus::InProgress, Some("worker"));
    active.name = "active dependency".into();
    let mut waiting = task("pending_waiting", TaskStatus::Pending, None);
    waiting.depends_on = vec![active.id.clone()];
    waiting.status_changed_at = 1;
    let mut missing = task("pending_missing", TaskStatus::Pending, None);
    missing.depends_on = vec!["dep_missing".into()];
    missing.status_changed_at = 1;
    let mut actionable = task("pending_free", TaskStatus::Pending, None);
    actionable.name = "orphan work".into();
    actionable.status_changed_at = 1;
    plan(
        "dependency-reminder",
        "orch",
        PlanMode::Execution,
        vec![active, waiting, missing, actionable],
    )
}

fn fixture_task<'a>(plan: &'a Plan, id: &str) -> &'a Task {
    plan.tasks.iter().find(|task| task.id == id).unwrap()
}

fn is_aged(plan: &Plan, id: &str) -> bool {
    crate::plan::reconcile::aged_open_tasks(plan, AGED_NOW_MS)
        .iter()
        .any(|item| item.task_id == id)
}

fn has_stale_unstaffed_drift(plan: &Plan, id: &str) -> bool {
    crate::plan::reconcile::detect_plan_drift(
        plan,
        &crate::plan::reconcile::ReconcileConfig::new(AGED_NOW_MS),
        |_| crate::plan::reconcile::TabProbe::default(),
    )
    .iter()
    .any(|item| {
        item.task_id == id && item.kind == crate::plan::reconcile::DriftKind::StalePendingUnstaffed
    })
}

fn reminder_key(plan: &Plan) -> String {
    plan_reminder_state_key(
        &plan.id,
        crate::service::plan::reminders::NudgeKind::Orchestrator,
        plan,
    )
}

#[test]
fn dependency_waiting_pending_is_not_stale_unstaffed_or_aged() {
    let plan = dependency_fixture();
    let waiting = fixture_task(&plan, "pending_waiting");
    let projection = task_projection(&plan, waiting);
    assert_eq!(
        projection.presentation,
        TaskPresentation::WaitingOnDependencies
    );
    assert_eq!(projection.dependency_waits.len(), 1);
    assert_eq!(projection.dependency_waits[0].task_id, "dep_active");
    assert_eq!(
        projection.dependency_waits[0].status,
        Some(TaskStatus::InProgress)
    );
    let waiting_item = crate::plan::reconcile::AgedOpenTask {
        task_id: waiting.id.clone(),
        name: waiting.name.clone(),
        status: waiting.status.clone(),
        presentation: projection.presentation,
        dependency_waits: projection.dependency_waits,
        age_ms: AGED_NOW_MS,
    };
    assert_eq!(
        aged_task_status_text(&waiting_item, Some(waiting)),
        "waiting on dependencies: dep_active (in_progress)"
    );

    assert!(!is_aged(&plan, &waiting.id));
    assert!(!has_stale_unstaffed_drift(&plan, &waiting.id));
    assert!(!aged_task_callout_text(&plan, AGED_NOW_MS)
        .unwrap()
        .contains(&waiting.id));
}

#[test]
fn missing_dependency_remains_waiting_and_is_not_stale_actionable() {
    let plan = dependency_fixture();
    let missing = fixture_task(&plan, "pending_missing");
    let projection = task_projection(&plan, missing);
    assert_eq!(
        projection.presentation,
        TaskPresentation::WaitingOnDependencies
    );
    assert_eq!(projection.dependency_waits.len(), 1);
    assert_eq!(projection.dependency_waits[0].task_id, "dep_missing");
    assert!(projection.dependency_waits[0].missing);
    assert_eq!(projection.dependency_waits[0].status, None);

    let waiting_item = crate::plan::reconcile::AgedOpenTask {
        task_id: missing.id.clone(),
        name: missing.name.clone(),
        status: missing.status.clone(),
        presentation: projection.presentation,
        dependency_waits: projection.dependency_waits,
        age_ms: AGED_NOW_MS,
    };
    assert_eq!(
        aged_task_status_text(&waiting_item, Some(missing)),
        "waiting on dependencies: dep_missing (missing)"
    );
    assert!(!is_aged(&plan, &missing.id));
    assert!(!has_stale_unstaffed_drift(&plan, &missing.id));
    let waiting_key = reminder_key(&plan);
    let mut dependency_free = plan.clone();
    dependency_free
        .tasks
        .iter_mut()
        .find(|task| task.id == missing.id)
        .unwrap()
        .depends_on
        .clear();
    assert_ne!(waiting_key, reminder_key(&dependency_free));
}

#[test]
fn dependency_waiting_pending_is_not_dispatched() {
    let plan = dependency_fixture();
    let live_tabs = std::collections::HashSet::from(["worker".to_string()]);
    let snapshot = codeterm_plan::service_plan::state::build_plan_state_snapshot(
        &plan,
        crate::plan::coverage::CoverageReport {
            plan_covered_fraction: 0.0,
            sections: vec![],
            gaps: vec![],
            unlinked_count: 0,
            unlinked_tasks: vec![],
        },
        vec![],
        vec![],
        vec![],
        &live_tabs,
        None,
        AGED_NOW_MS,
    );

    assert!(snapshot.next_actions.iter().all(|action| {
        !matches!(
            action.kind,
            codeterm_plan::service_plan::next_actions::NextActionKind::Continue
        ) && !(action.kind == codeterm_plan::service_plan::next_actions::NextActionKind::Dispatch
            && matches!(
                action.task_id.as_deref(),
                Some("pending_waiting" | "pending_missing")
            ))
    }));
}

#[test]
fn dependency_free_aged_pending_remains_stale_unstaffed() {
    let plan = dependency_fixture();
    let actionable = fixture_task(&plan, "pending_free");
    assert_eq!(
        task_projection(&plan, actionable).presentation,
        TaskPresentation::ActionablePending
    );
    assert!(is_aged(&plan, &actionable.id));
    assert!(has_stale_unstaffed_drift(&plan, &actionable.id));
    let callout = aged_task_callout_text(&plan, AGED_NOW_MS).unwrap();
    assert_eq!(callout.matches("pending_free").count(), 1);
    assert!(callout.contains("actionable pending"));
}

#[test]
fn dependency_completion_changes_reminder_state_key_without_status_change() {
    let mut plan = dependency_fixture();
    let before = reminder_key(&plan);
    assert_eq!(before, reminder_key(&plan));
    assert!(!aged_task_callout_text(&plan, AGED_NOW_MS)
        .unwrap()
        .contains("pending_waiting"));

    plan.tasks
        .iter_mut()
        .find(|task| task.id == "dep_active")
        .unwrap()
        .status = TaskStatus::Done;
    let waiting = fixture_task(&plan, "pending_waiting");
    assert_eq!(waiting.status, TaskStatus::Pending);
    assert_eq!(
        task_projection(&plan, waiting).presentation,
        TaskPresentation::ActionablePending
    );
    let after = reminder_key(&plan);
    assert_ne!(before, after);
    assert_eq!(after, reminder_key(&plan));
    assert!(aged_task_callout_text(&plan, AGED_NOW_MS)
        .unwrap()
        .contains("pending_waiting"));
}

fn owner_plan(tab_id: &str) -> Plan {
    plan(
        "plan-reminder-test",
        tab_id,
        PlanMode::Execution,
        vec![task("open", TaskStatus::Pending, None)],
    )
}

#[test]
fn reminder_state_key_ignores_bookkeeping_timestamp_changes() {
    let mut plan = owner_plan("orch");
    let before = plan_reminder_state_key(
        &plan.id,
        crate::service::plan::reminders::NudgeKind::Orchestrator,
        &plan,
    );
    plan.updated_at = 999_999;
    let after = plan_reminder_state_key(
        &plan.id,
        crate::service::plan::reminders::NudgeKind::Orchestrator,
        &plan,
    );

    assert_eq!(before, after);
}

#[test]
fn event_paths_have_no_plan_reminder_injection() {
    let cases = [
        (
            "report completion",
            include_str!("../../hierarchy.rs"),
            concat!("maybe_send_", "plan_reminder("),
        ),
        (
            "HTTP owned-tab send",
            include_str!("../../../api/tab_routes/send.rs"),
            concat!("maybe_send_owned_", "plan_reminder("),
        ),
        (
            "socket owned-tab send",
            include_str!("../../../socket/server/tab.rs"),
            concat!("maybe_send_owned_", "plan_reminder("),
        ),
        (
            "HTTP text polling",
            include_str!("../../../api/tab_routes.rs"),
            concat!("maybe_send_polling_", "nudge("),
        ),
        (
            "HTTP chat polling",
            include_str!("../../../api/tab_chat_routes.rs"),
            concat!("maybe_send_polling_", "nudge("),
        ),
        (
            "socket output polling",
            include_str!("../../../socket/server/tab.rs"),
            concat!("maybe_send_polling_", "nudge("),
        ),
        (
            "idle transition",
            include_str!("../../../startup/background.rs"),
            concat!("maybe_send_idle_", "plan_poke("),
        ),
    ];

    for (event, source, obsolete_call) in cases {
        assert!(
            !source.contains(obsolete_call),
            "{event} must not inject a periodic plan reminder"
        );
    }

    let hierarchy = include_str!("../../hierarchy.rs");
    assert!(hierarchy.contains("report.inject_body()"));
    assert!(hierarchy.contains("pub fn notify_owner_task_in_review"));
    assert!(hierarchy.contains("pub fn nudge_gate_asker"));
}

#[test]
fn reminder_role_only_shell_records_failure_without_pty_write_or_nudge() {
    let (mux, tab_id) = reminder_test_tab();
    let counter = crate::service::plan::reminders::TurnCounter::new();
    let writes = RefCell::new(Vec::new());
    let plans = vec![owner_plan(&tab_id)];
    assert!(!counter.time_nudge_due(&tab_id, 90, 10));

    send_due_plan_time_reminders_with(&plans, &counter, 100, 10, |id, _, _| {
        if crate::service::plan_reconcile::tab_has_live_agent(mux.get_tab(id).unwrap()) {
            writes.borrow_mut().push(id.to_string());
            Ok(())
        } else {
            Err("agent_unavailable: plan reminder target has no verified live agent session".into())
        }
    });

    let telemetry = counter.time_reminder_telemetry(&tab_id);
    assert!(writes.borrow().is_empty());
    assert!(telemetry.last_due.is_some());
    assert!(telemetry.last_delivery.is_none());
    assert!(telemetry
        .delivery_failure_retry_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("verified live agent session")));
    assert!(counter.time_nudge_due(&tab_id, 100, 10));
}

#[test]
fn scheduler_delivers_at_1800_seconds_once_and_marks_nudged() {
    let (mut mux, tab_id) = reminder_test_tab();
    let counter = crate::service::plan::reminders::TurnCounter::new();
    let plans = vec![owner_plan(&tab_id)];
    let interval =
        codeterm_types::config::schema::OrchestrationConfig::default().nudge_interval_secs;
    assert_eq!(
        interval,
        codeterm_types::config::schema::OrchestrationConfig::default().nudge_interval_secs
    );
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("test-provider".into());
        tab.agent_session_id = Some("session-1".into());
        tab.session_binding_nonce = Some("nonce-1".into());
        tab.reported_session_binding_nonce = Some("nonce-1".into());
        tab.pty_id = Some("recording-pty".into());
        tab.chat_view = true;
        tab.boot_stage = Some(BootStage::AgentReady);
    }
    let writes = RefCell::new(Vec::new());
    assert!(!counter.time_nudge_due(&tab_id, 0, interval));

    for (now, expected_deliveries) in [(interval - 1, 0), (interval, 1), (interval, 1)] {
        send_due_plan_time_reminders_with(&plans, &counter, now, interval, |id, _, _| {
            assert!(crate::service::plan_reconcile::tab_has_live_agent(
                mux.get_tab(id).unwrap()
            ));
            writes.borrow_mut().push(id.to_string());
            Ok(())
        });
        assert_eq!(writes.borrow().len(), expected_deliveries);
    }

    let telemetry = counter.time_reminder_telemetry(&tab_id);
    assert_eq!(writes.borrow().len(), 1);
    assert!(telemetry.last_delivery.is_some());
    assert!(telemetry.delivery_failure_retry_reason.is_none());
}

#[test]
fn time_targets_include_execution_owner_and_worker() {
    let plans = vec![plan(
        "pl_1",
        "orch",
        PlanMode::Execution,
        vec![task("task_1", TaskStatus::Pending, Some("worker"))],
    )];

    assert_eq!(
        timed_plan_reminder_targets(&plans),
        vec![
            TimedPlanReminderTarget {
                tab_id: "orch".into(),
                target: PlanReminderTarget::Owner {
                    plan_id: "pl_1".into()
                }
            },
            TimedPlanReminderTarget {
                tab_id: "worker".into(),
                target: PlanReminderTarget::Worker
            }
        ]
    );
}

#[test]
fn time_targets_skip_planning_and_terminal_work() {
    let plans = vec![
        plan(
            "pl_planning",
            "orch",
            PlanMode::Planning,
            vec![task("task_1", TaskStatus::Pending, Some("worker"))],
        ),
        plan(
            "pl_done",
            "done-orch",
            PlanMode::Execution,
            vec![task("task_2", TaskStatus::Done, Some("done-worker"))],
        ),
        plan(
            "pl_cancelled",
            "cancelled-orch",
            PlanMode::Execution,
            vec![task(
                "task_3",
                TaskStatus::Cancelled,
                Some("cancelled-worker"),
            )],
        ),
    ];

    assert!(timed_plan_reminder_targets(&plans).is_empty());
}

#[test]
fn time_targets_preserve_pause_completion_and_closure_suppression() {
    let mut paused = plan(
        "pl_paused",
        "paused-owner",
        PlanMode::Execution,
        vec![task("open", TaskStatus::Pending, Some("paused-worker"))],
    );
    paused.mode = PlanMode::Paused;

    let paused_task = plan(
        "pl_paused_task",
        "paused-task-owner",
        PlanMode::Execution,
        vec![task("paused", TaskStatus::Paused, Some("paused-worker"))],
    );

    let mut archived = plan(
        "pl_closed",
        "closed-owner",
        PlanMode::Execution,
        vec![task("open", TaskStatus::Pending, Some("closed-worker"))],
    );
    archived.status = PlanStatus::Archived;

    let no_open_tasks = plan(
        "pl_complete",
        "complete-owner",
        PlanMode::Execution,
        vec![task("done", TaskStatus::Done, Some("done-worker"))],
    );

    assert!(
        timed_plan_reminder_targets(&[paused, paused_task, archived, no_open_tasks]).is_empty()
    );
}

#[test]
fn all_paused_backlog_stays_active_without_reminders_until_new_work_arrives() {
    let mut backlog = plan(
        "pl_backlog",
        "backlog-owner",
        PlanMode::Execution,
        vec![
            task("paused-1", TaskStatus::Paused, Some("worker-1")),
            task("paused-2", TaskStatus::Paused, Some("worker-2")),
            task("paused-3", TaskStatus::Paused, None),
            task("paused-4", TaskStatus::Paused, Some("worker-4")),
        ],
    );

    assert_eq!(backlog.status, PlanStatus::Active);
    assert_eq!(backlog.mode, PlanMode::Execution);
    assert!(!is_plan_complete(&backlog));
    assert!(timed_plan_reminder_targets(&[backlog.clone()]).is_empty());

    backlog
        .tasks
        .push(task("new-work", TaskStatus::Pending, None));

    assert_eq!(
        timed_plan_reminder_targets(&[backlog]),
        vec![TimedPlanReminderTarget {
            tab_id: "backlog-owner".into(),
            target: PlanReminderTarget::Owner {
                plan_id: "pl_backlog".into(),
            },
        }]
    );
}

#[test]
fn time_targets_prioritize_owner_over_worker_for_same_tab() {
    let plans = vec![plan(
        "pl_1",
        "orch",
        PlanMode::Execution,
        vec![task("task_1", TaskStatus::Pending, Some("orch"))],
    )];

    assert_eq!(
        timed_plan_reminder_targets(&plans),
        vec![TimedPlanReminderTarget {
            tab_id: "orch".into(),
            target: PlanReminderTarget::Owner {
                plan_id: "pl_1".into()
            }
        }]
    );
}

#[test]
fn time_targets_follow_reseeded_owner_tab() {
    let mut reseeded = plan(
        "pl_1",
        "stale-owner",
        PlanMode::Execution,
        vec![task("task_1", TaskStatus::InReview, None)],
    );
    reseeded.owner = OrchestratorRef::from_tab("live-owner");

    assert_eq!(
        timed_plan_reminder_targets(&[reseeded]),
        vec![TimedPlanReminderTarget {
            tab_id: "live-owner".into(),
            target: PlanReminderTarget::Owner {
                plan_id: "pl_1".into()
            }
        }]
    );
}

#[test]
fn time_targets_workers_only_for_report_pending_statuses() {
    let plans = vec![plan(
        "pl_1",
        "orch",
        PlanMode::Execution,
        vec![
            task("pending", TaskStatus::Pending, Some("p1")),
            task("working", TaskStatus::InProgress, Some("p2")),
            task("blocked", TaskStatus::Blocked, Some("p3")),
            task("review", TaskStatus::InReview, Some("p4")),
        ],
    )];
    let targets = timed_plan_reminder_targets(&plans);

    assert!(targets.iter().any(|target| target.tab_id == "p1"));
    assert!(targets.iter().any(|target| target.tab_id == "p2"));
    assert!(targets.iter().any(|target| target.tab_id == "p3"));
    assert!(!targets.iter().any(|target| target.tab_id == "p4"));
}

#[test]
fn timed_targets_silent_for_all_done_and_complete_mode_plans() {
    // Execution plan where all tasks are Done — no nudges for owner or workers.
    let all_done = plan(
        "pl_done",
        "orch-done",
        PlanMode::Execution,
        vec![
            task("t1", TaskStatus::Done, Some("w1")),
            task("t2", TaskStatus::Done, Some("w2")),
            task("t3", TaskStatus::Cancelled, Some("w3")),
        ],
    );
    // Complete mode plan with open-looking task — still silent.
    let mut complete_mode = plan(
        "pl_complete",
        "orch-complete",
        PlanMode::Execution,
        vec![task("t4", TaskStatus::Done, None)],
    );
    complete_mode.mode = PlanMode::Complete;

    let targets = timed_plan_reminder_targets(&[all_done, complete_mode]);
    assert!(
        targets.is_empty(),
        "complete plans must produce zero timed reminder targets: {targets:?}"
    );
}

#[test]
fn timed_targets_fire_for_in_review_open_work() {
    // InReview is open work: plan is NOT complete, owner should be nudged.
    let plans = vec![plan(
        "pl_review",
        "orch-review",
        PlanMode::Execution,
        vec![
            task("t_done", TaskStatus::Done, None),
            task("t_review", TaskStatus::InReview, Some("worker-review")),
        ],
    )];
    let targets = timed_plan_reminder_targets(&plans);
    assert!(
        targets.iter().any(|t| t.tab_id == "orch-review"),
        "in_review open work must keep plan non-complete and owner nudge-eligible"
    );
}

fn owner_target(plan: &Plan) -> bool {
    timed_plan_reminder_targets(std::slice::from_ref(plan))
        .iter()
        .any(|target| {
            target.target
                == PlanReminderTarget::Owner {
                    plan_id: plan.id.clone(),
                }
        })
}

fn held_dependency_plan() -> Plan {
    let mut held = task("dep_held", TaskStatus::Paused, None);
    held.pause_reason = Some("gate:g_1: owner decision".into());
    let mut waiting = task(
        "pending_waiting",
        TaskStatus::Pending,
        Some("waiting-worker"),
    );
    waiting.depends_on = vec![held.id.clone()];
    plan("pl_wait", "orch", PlanMode::Execution, vec![held, waiting])
}

#[test]
fn plan_reminder_dependency_wait_alone_creates_no_actionable_nudge() {
    let plan = held_dependency_plan();
    assert!(!is_plan_complete(&plan));
    assert!(plan
        .tasks
        .iter()
        .any(|task| task.status.counts_as_open_work()));
    assert!(timed_plan_reminder_targets(&[plan]).is_empty());
}

#[test]
fn plan_reminder_dependency_wait_keeps_actionable_statuses_nudged() {
    for status in [
        TaskStatus::Pending,
        TaskStatus::InProgress,
        TaskStatus::Blocked,
    ] {
        let mut plan = held_dependency_plan();
        plan.tasks.push(task("task_ready", status.clone(), None));
        assert!(owner_target(&plan), "{status:?} must keep the owner nudged");
        let text = crate::service::plan::reminders::orchestrator_nudge_text(&plan);
        assert!(text.contains("task_ready"), "{status:?} task must be named");
        assert!(!text.contains("pending_waiting"));
        assert!(!text.contains("dep_held"));
        assert!(!text.contains("{actionable}"));
    }
}

#[test]
fn plan_reminder_dependency_wait_on_missing_task_stays_actionable() {
    let mut orphan = task("pending_orphan", TaskStatus::Pending, None);
    orphan.depends_on = vec!["task_gone".into()];
    let plan = plan("pl_missing", "orch", PlanMode::Execution, vec![orphan]);
    assert!(owner_target(&plan));
    assert!(
        crate::service::plan::reminders::orchestrator_nudge_text(&plan).contains("pending_orphan")
    );
}

#[test]
fn plan_reminder_dependency_wait_skips_the_waiting_worker_only() {
    let mut plan = held_dependency_plan();
    plan.tasks.push(task(
        "task_ready",
        TaskStatus::InProgress,
        Some("live-worker"),
    ));
    let targets = timed_plan_reminder_targets(&[plan]);
    assert!(targets.iter().any(|target| target.tab_id == "live-worker"));
    assert!(!targets
        .iter()
        .any(|target| target.tab_id == "waiting-worker"));
}

#[test]
fn plan_reminder_dependency_wait_becomes_actionable_when_the_dependency_lands() {
    let mut plan = held_dependency_plan();
    plan.tasks[0].status = TaskStatus::Done;
    assert!(owner_target(&plan));
    let text = crate::service::plan::reminders::orchestrator_nudge_text(&plan);
    assert!(text.contains("pending_waiting"));
    assert!(!text.contains("dep_held"));
}
