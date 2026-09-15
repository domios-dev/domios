use codeterm_plan::service_plan::state::mission_control::{
    AreaFacetState, KpiLedgerState, KpiNotRecordingReason,
};

use crate::agent::kpi::KpiRecorder;
use crate::agent::kpi_ledger::{KpiEventInput, KpiEventKind, KpiLedger, TokenCounts};
use crate::plan::coverage::compute_coverage_report;
use crate::plan::{
    AgentRef, DecisionGate, GateStatus, Milestone, OrchestratorRef, Plan, PlanStatus, PlannerRef,
    PlanningAuthor, PlanningState, PlanningTransition, TagFacet, TagFacetValue,
    TaskExecutionAttempt, TaskStatus,
};
use crate::service::plan::{build_plan_state_snapshot, NewTask, PlanStateSnapshot, PlanStore};

const NOW: u64 = 10_000;

fn milestone(id: &str, kind: &str, text: &str, ts: u64, tab_id: &str) -> Milestone {
    Milestone {
        id: id.into(),
        from: AgentRef {
            tab_id: Some(tab_id.into()),
            session_id: None,
            title: None,
        },
        kind: kind.into(),
        text: text.into(),
        ts,
        links: vec![],
        loop_id: None,
        iteration: None,
        canvas: Default::default(),
        attachments: vec![],
    }
}

fn attempt(id: &str, tab_id: &str, title: &str) -> TaskExecutionAttempt {
    TaskExecutionAttempt {
        id: id.into(),
        executor_tab_id: tab_id.into(),
        executor_title: Some(title.into()),
        ..Default::default()
    }
}

/// A plan carrying every stored field the projection reads: a held review with
/// two verdicts, a bounced label, a second attempt, a run of worker reports by
/// one executor, a plan-level stride, an open gate and a planning transition.
/// Tasks are minted through the store, then stamped on the owned copy the same
/// way the review, dispatch and gate paths stamp them.
fn seeded_plan(root: &std::path::Path) -> Plan {
    let store = PlanStore::new_for_test(root);
    let mut plan = Plan::new(
        "Mission control".into(),
        root.display().to_string(),
        OrchestratorRef {
            tab_id: Some("tab-orch".into()),
            session_id: None,
            title: Some("Planck".into()),
        },
    );
    plan.status = PlanStatus::Active;
    plan.planner = Some(PlannerRef::Tab {
        tab_id: "tab-planner".into(),
        session_id: None,
        title: Some("Pasteur".into()),
    });
    store.save(&mut plan, 1).unwrap();
    for (name, role, created_at) in [
        ("Held task", Some("backend"), 100),
        ("Shipped task", Some("frontend"), 110),
        ("Bounced task", None, 120),
    ] {
        store
            .add_task(
                &plan.id,
                NewTask {
                    name: name.into(),
                    kind: "implement".into(),
                    executor_role: role.map(str::to_string),
                    ..Default::default()
                },
                created_at,
            )
            .unwrap();
    }

    let mut plan = store.get(&plan.id).unwrap();
    plan.planning_log.push(PlanningTransition {
        from: PlanningState::Drafting,
        to: PlanningState::StructureReady,
        by: PlanningAuthor::Planner {
            tab_id: "tab-planner".into(),
        },
        note: None,
        at: 150,
    });
    plan.gates.push(DecisionGate {
        id: "gt_1".into(),
        question: "Ship the projection now?\nsecond line".into(),
        context: String::new(),
        blocking_task_ids: vec![plan.tasks[0].id.clone(), plan.tasks[2].id.clone()],
        routed_to_tab_id: None,
        asked_by_tab_id: Some("tab-orch".into()),
        asked_at: 300,
        status: GateStatus::Open,
        answer: None,
        escalated_at: None,
        escalation_note: None,
    });
    plan.strides.push(milestone(
        "ms_stride",
        "skill",
        "kept verify-before-claim",
        400,
        "tab-orch",
    ));

    let held = &mut plan.tasks[0];
    held.status = TaskStatus::InReview;
    held.status_label = Some("review held".into());
    held.execution_attempts = vec![
        attempt("at_1", "tab-w1", "Rowan"),
        attempt("at_2", "tab-w1", "Rowan"),
    ];
    held.milestones = vec![
        milestone("ms_r1", "worker_report", "STATUS: in_review", 500, "tab-w1"),
        milestone("ms_r2", "worker_report", "STATUS: in_review", 510, "tab-w1"),
        milestone("ms_r3", "worker_report", "STATUS: in_review", 520, "tab-w1"),
        milestone("ms_v1", "review", "Bounce: needs evidence", 600, "tab-orch"),
        milestone(
            "ms_v2",
            "review",
            "Hold: waiting on a decision",
            900,
            "tab-orch",
        ),
    ];

    let shipped = &mut plan.tasks[1];
    shipped.status = TaskStatus::Done;
    shipped.execution_attempts = vec![attempt("at_3", "tab-w2", "Fermi")];

    let bounced = &mut plan.tasks[2];
    bounced.status = TaskStatus::InProgress;
    bounced.status_label = Some("review bounced".into());
    bounced.execution_attempts = vec![attempt("at_4", "tab-w3", "Bohr")];
    plan
}

fn snapshot_of(plan: &Plan) -> PlanStateSnapshot {
    let coverage = compute_coverage_report(plan);
    build_plan_state_snapshot(
        plan,
        coverage,
        vec![],
        vec![],
        vec![],
        &Default::default(),
        None,
        NOW,
    )
}

#[test]
fn mission_control_open_gate_names_the_routed_tab_before_the_asking_tab() {
    let temp = tempfile::tempdir().unwrap();
    let mut plan = seeded_plan(temp.path());
    plan.gates[0].routed_to_tab_id = Some("tab-hopper".into());
    let gates = snapshot_of(&plan).mission_control.held.open_gates;
    assert_eq!(gates[0].tab_id.as_deref(), Some("tab-hopper"));

    plan.gates[0].routed_to_tab_id = None;
    plan.gates[0].asked_by_tab_id = None;
    let gates = snapshot_of(&plan).mission_control.held.open_gates;
    assert_eq!(gates[0].tab_id, None);
}

#[test]
fn mission_control_state_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let plan = seeded_plan(temp.path());
    let mission = snapshot_of(&plan).mission_control;

    let held = &mission.held;
    assert_eq!(held.held_reviews.len(), 1, "only the stored label holds");
    assert_eq!(held.held_reviews[0].name, "Held task");
    assert_eq!(held.held_reviews[0].verdicts, 2);
    assert_eq!(held.held_reviews[0].last_verdict_at, Some(900));
    assert_eq!(held.held_reviews[0].age_ms, Some(NOW - 900));
    assert_eq!(held.oldest_hold_age_ms, Some(NOW - 900));
    assert_eq!(held.verdicts_on_held, 2);
    assert_eq!(held.open_gates.len(), 1);
    assert_eq!(held.open_gates[0].blocking_task_count, 2);
    assert_eq!(held.open_gates[0].age_ms, NOW - 300);
    assert_eq!(held.open_gates[0].tab_id.as_deref(), Some("tab-orch"));

    let bounces = &mission.bounces;
    assert_eq!(bounces.total_attempts, 4);
    assert_eq!(bounces.tasks_on_second_attempt, 1);
    assert_eq!(bounces.first_try_done, 1);
    assert_eq!(bounces.labelled_bounced, 1);

    let log = &mission.event_log;
    // 1 planning + 3 task_created + 5 milestones + 1 stride + 1 gate.
    assert_eq!(log.recorded_events, 11);
    assert!(!log.truncated);
    let stamps: Vec<u64> = log.entries.iter().map(|entry| entry.ts).collect();
    let mut ordered = stamps.clone();
    ordered.sort_unstable();
    assert_eq!(stamps, ordered, "events are time-ordered");

    let reports: Vec<_> = log
        .entries
        .iter()
        .filter(|entry| entry.kind == "worker_report")
        .collect();
    assert_eq!(
        reports.len(),
        1,
        "one agent on one task collapses to a line"
    );
    assert_eq!(reports[0].count, 3);
    assert_eq!(reports[0].ts, 500);
    assert_eq!(reports[0].last_ts, 520);
    assert_eq!(reports[0].author, "Rowan", "author is the recorded title");

    let stride = log
        .entries
        .iter()
        .find(|entry| entry.kind == "skill")
        .unwrap();
    assert_eq!(stride.author, "Planck");
    assert!(stride.task_id.is_none());

    let gate = log
        .entries
        .iter()
        .find(|entry| entry.kind == "gate")
        .unwrap();
    assert_eq!(
        gate.text, "Ship the projection now?",
        "first line as written"
    );
}

#[test]
fn mission_control_state_snapshot_role_fallback_without_area_facet() {
    let temp = tempfile::tempdir().unwrap();
    let plan = seeded_plan(temp.path());
    let areas = snapshot_of(&plan).mission_control.areas;

    assert_eq!(areas.state, AreaFacetState::NoFacet);
    assert!(areas.facet_key.is_none());
    assert!(areas.groups.is_empty());
    assert_eq!(areas.untagged_task_count, 3);
    let roles: Vec<Option<String>> = areas
        .role_fallback
        .iter()
        .map(|group| group.executor_role.clone())
        .collect();
    assert_eq!(
        roles,
        vec![None, Some("backend".into()), Some("frontend".into())]
    );
}

#[test]
fn mission_control_state_snapshot_groups_by_declared_area_facet() {
    let temp = tempfile::tempdir().unwrap();
    let mut plan = seeded_plan(temp.path());
    plan.tag_facets = vec![TagFacet {
        key: "area".into(),
        label: Some("Area".into()),
        color_lane: false,
        values: vec![
            TagFacetValue {
                v: "backend".into(),
                label: Some("Backend".into()),
                color: None,
            },
            TagFacetValue {
                v: "frontend".into(),
                label: None,
                color: None,
            },
        ],
    }];
    plan.tasks[0].tags = vec!["area:backend".into(), "lane".into()];
    plan.tasks[1].tags = vec!["area:frontend".into()];
    let areas = snapshot_of(&plan).mission_control.areas;

    assert_eq!(areas.state, AreaFacetState::Declared);
    assert_eq!(areas.facet_key.as_deref(), Some("area"));
    assert_eq!(areas.facet_label.as_deref(), Some("Area"));
    assert!(areas.role_fallback.is_empty());
    assert_eq!(areas.untagged_task_count, 1);
    assert_eq!(areas.groups.len(), 2);
    assert_eq!(areas.groups[0].label, "Backend");
    assert_eq!(areas.groups[0].task_count, 1);
    assert_eq!(
        areas.groups[1].label, "frontend",
        "value stands in for a missing label"
    );
    assert_eq!(areas.groups[1].task_count, 1);
}

#[test]
fn mission_control_state_snapshot_preserves_existing_consumers() {
    let temp = tempfile::tempdir().unwrap();
    let plan = seeded_plan(temp.path());
    let snapshot = snapshot_of(&plan);
    let json = serde_json::to_value(&snapshot).unwrap();

    for key in [
        "plan_id",
        "task_counts",
        "percent_done",
        "open_lanes",
        "blocked_lanes",
        "recent_strides",
        "review_gate",
        "next_actions",
    ] {
        assert!(json.get(key).is_some(), "{key} must survive the addition");
    }
    assert_eq!(snapshot.task_counts.total, 3);
    assert_eq!(json["mission_control"]["kpi"]["state"], "not_recording");
}

fn ledger_event(plan_id: &str, kind: KpiEventKind, tokens: Option<TokenCounts>) -> KpiEventInput {
    KpiEventInput {
        plan_id: Some(plan_id.into()),
        task_id: None,
        tab_id: "tab-w1".into(),
        role: None,
        provider: "claude".into(),
        model: None,
        timestamp: 1,
        kind,
        bytes: 120,
        tokens,
        dedup_key: None,
        workspace_id: None,
        tab_kind: None,
    }
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn mission_control_state_endpoint_uses_ledger_without_jsonl_scan() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let mut fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let mut plan = Plan::new(
        "Ledger backed".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef {
            tab_id: Some("tab-orch".into()),
            session_id: None,
            title: Some("Planck".into()),
        },
    );
    fixture.service.plan_store.save(&mut plan, 1).unwrap();

    // The flag is off in the fixture: no ledger, so no figure and no other source.
    let kpi = fixture
        .service
        .build_plan_state(&plan.id, NOW)
        .unwrap()
        .mission_control
        .kpi;
    assert_eq!(kpi.state, KpiLedgerState::NotRecording);
    assert_eq!(
        kpi.not_recording_reason,
        Some(KpiNotRecordingReason::Disabled)
    );
    assert!(kpi.spend.is_none());

    // Ledger on but holding nothing for this plan: still not recording, never a
    // fallback scan of provider session logs.
    let ledger = std::sync::Arc::new(
        KpiLedger::open_at(&fixture.temp.path().join("kpi_ledger.db")).unwrap(),
    );
    ledger
        .insert(&ledger_event("pl_other", KpiEventKind::Send, None))
        .unwrap();
    fixture.service.kpi = KpiRecorder::with_ledger(ledger.clone());
    let kpi = fixture
        .service
        .build_plan_state(&plan.id, NOW)
        .unwrap()
        .mission_control
        .kpi;
    assert_eq!(kpi.state, KpiLedgerState::NotRecording);
    assert_eq!(
        kpi.not_recording_reason,
        Some(KpiNotRecordingReason::NoRowsForPlan)
    );
    assert!(kpi.spend.is_none());

    ledger
        .insert(&ledger_event(&plan.id, KpiEventKind::Send, None))
        .unwrap();
    ledger
        .insert(&ledger_event(
            &plan.id,
            KpiEventKind::TokenUsage,
            Some(TokenCounts {
                input: 900,
                uncached_input: 100,
                output: 250,
            }),
        ))
        .unwrap();

    let snapshot = fixture.service.build_plan_state(&plan.id, NOW).unwrap();
    let spend = snapshot.mission_control.kpi.spend.unwrap();
    let row = ledger
        .aggregate_by_plan()
        .unwrap()
        .into_iter()
        .find(|row| row.plan_id.as_deref() == Some(plan.id.as_str()))
        .unwrap();
    assert_eq!(spend.events, row.count);
    assert_eq!(spend.bytes, row.total_bytes);
    assert_eq!(spend.turns, row.turns, "turns count token_usage rows only");
    assert_eq!(spend.turns, 1);
    assert_eq!(spend.tokens.input, 900);
    assert_eq!(spend.tokens.uncached_input, 100);
    assert_eq!(spend.tokens.output, 250);
    assert_eq!(spend.tokens.total, 1150);

    // The route's own payload: the snapshot serialized as the state handler sends it.
    let payload = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(payload["mission_control"]["kpi"]["state"], "recording");
    assert_eq!(payload["mission_control"]["kpi"]["spend"]["turns"], 1);
    assert!(payload["mission_control"]["spend"]["state"].is_string());
    assert_eq!(payload["mission_control"]["spend"]["scope"], "machine");
}

/// The plan the live Board proof runs against; the aggregate here is written by the
/// same hooks that plan's team drives, not by a hand-built ledger row.
const LIVE_PLAN_ID: &str = "pl_ad3tcu";

fn live_plan(fixture: &crate::service::test_fixture::ServiceFixture) -> Plan {
    let mut plan = Plan::new(
        "CodeTerm 1.10.17 verification".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef {
            tab_id: Some("tab-orch".into()),
            session_id: None,
            title: Some("Planck".into()),
        },
    );
    plan.id = LIVE_PLAN_ID.into();
    plan.status = PlanStatus::Active;
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    plan
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn enabled_kpi_hooks_produce_plan_token_and_turn_aggregate() {
    use crate::agent::test_provider::MockProvider;
    use crate::multiplexer::types::AgentRole;
    use crate::service::messenger::MessageKind;

    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let mut fixture = crate::service::test_fixture::ServiceFixture::new(mux);

    // The provider owns its session log; the host reaches it only through the capability.
    let session = fixture.temp.path().join("session.jsonl");
    std::fs::write(
        &session,
        include_str!("../../agent/kpi/usage/fixtures/claude-session.jsonl"),
    )
    .unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(MockProvider::with_session_file(
            "kpi-mock",
            session.clone(),
        )));

    let workspace = fixture
        .service
        .mux
        .write()
        .create_workspace("kpi".into(), None)
        .id;
    let tab_id = fixture.service.mux.write().create_tab(&workspace).unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("kpi-mock".into());
        tab.cwd = Some(fixture.temp.path().display().to_string());
        tab.agent_session_id = Some("sess-kpi".into());
        tab.role = Some(AgentRole::Worker);
    }

    let plan = live_plan(&fixture);
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "Make KPI recording available to Mission Control".into(),
                kind: "implement".into(),
                assigned_tab_id: Some(tab_id.clone()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    {
        let mut active = fixture.service.plan_store.get(&plan.id).unwrap();
        active.status = PlanStatus::Active;
        fixture.service.plan_store.save(&mut active, 3).unwrap();
    }
    assert_eq!(
        fixture.service.plan_store.task_binding_for_tab(&tab_id),
        Some((plan.id.clone(), task_id.clone())),
        "the hooks resolve attribution through this binding; without it the test proves nothing"
    );

    let ledger = std::sync::Arc::new(
        KpiLedger::open_at(&fixture.temp.path().join("kpi_ledger.db")).unwrap(),
    );
    fixture.service.kpi = KpiRecorder::with_ledger(ledger.clone());

    // The shipped choke points, called exactly as spawn, deliver, report and output do.
    fixture
        .service
        .record_kpi_spawn(&tab_id, Some("claude-opus-5"), 120);
    fixture
        .service
        .record_kpi_delivery(&tab_id, MessageKind::Message, 64);
    fixture.service.record_kpi_report(
        &tab_id,
        Some(plan.id.as_str()),
        Some(task_id.as_str()),
        Some("claude-opus-5"),
        48,
        "rep_kpi_1",
    );
    fixture.service.request_kpi_usage_ingest(&tab_id);
    fixture.service.kpi.drain();

    let row = ledger
        .aggregate_by_plan()
        .unwrap()
        .into_iter()
        .find(|row| row.plan_id.as_deref() == Some(LIVE_PLAN_ID))
        .expect("the hooks must attribute their rows to the plan bound to the tab");
    assert_eq!(row.turns, 3, "one turn per usage record the provider wrote");
    assert_eq!(row.output_tokens, 1800);
    assert_eq!(row.input_tokens, 514_028);
    assert_eq!(row.uncached_input_tokens, 25_600);
    assert_eq!(
        row.count, 7,
        "three choke-point events, three usage turns and one compaction"
    );

    let snapshot = fixture.service.build_plan_state(&plan.id, NOW).unwrap();
    let kpi = &snapshot.mission_control.kpi;
    assert_eq!(kpi.state, KpiLedgerState::Recording);
    assert_eq!(kpi.not_recording_reason, None);
    assert_eq!(
        kpi.enable_setting, None,
        "nothing to enable while recording"
    );
    let spend = kpi.spend.expect("a recording board carries a figure");
    assert_eq!(spend.turns, row.turns);
    assert_eq!(spend.tokens.output, row.output_tokens);
    assert_eq!(spend.tokens.input, row.input_tokens);
    assert_eq!(spend.tokens.uncached_input, row.uncached_input_tokens);
    assert_eq!(spend.tokens.total, row.input_tokens + row.output_tokens);
    assert_eq!(spend.events, row.count);
    assert!(
        spend.tokens.output > 0 && spend.turns > 0,
        "the Board reads real figures, not placeholders"
    );

    let payload = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(payload["mission_control"]["kpi"]["state"], "recording");
    assert_eq!(payload["mission_control"]["kpi"]["spend"]["turns"], 3);
    assert_eq!(
        payload["mission_control"]["kpi"]["spend"]["tokens"]["output"],
        1800
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn mission_control_not_recording_state_explains_enable_path() {
    use codeterm_plan::service_plan::state::mission_control::KPI_ENABLED_SETTING;

    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let mut fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let plan = live_plan(&fixture);

    let read_kpi = |fixture: &crate::service::test_fixture::ServiceFixture| {
        fixture
            .service
            .build_plan_state(&plan.id, NOW)
            .unwrap()
            .mission_control
            .kpi
    };

    // Opt-out: the only state a setting fixes, so it is the only state that names one.
    fixture.service.kpi = KpiRecorder::disabled();
    let kpi = read_kpi(&fixture);
    assert_eq!(kpi.state, KpiLedgerState::NotRecording);
    assert_eq!(
        kpi.not_recording_reason,
        Some(KpiNotRecordingReason::Disabled)
    );
    assert_eq!(kpi.enable_setting, Some(KPI_ENABLED_SETTING));
    assert!(kpi.spend.is_none());
    let json = serde_json::to_value(&kpi).unwrap();
    assert_eq!(json["enable_setting"], KPI_ENABLED_SETTING);
    assert!(
        json["spend"].is_null(),
        "a missing figure stays missing; it never becomes a zero"
    );

    // Configured on but the ledger would not open: telling the owner to switch it on
    // would be wrong, so the state reports the failure and names no setting.
    let blocked = fixture.temp.path().join("blocked-ledger-dir");
    std::fs::write(&blocked, b"not a directory").unwrap();
    let failed = KpiRecorder::from_config(&crate::config::schema::AppConfig::default(), &blocked);
    assert!(!failed.is_enabled());
    assert!(failed.open_failed());
    fixture.service.kpi = failed;
    let kpi = read_kpi(&fixture);
    assert_eq!(
        kpi.not_recording_reason,
        Some(KpiNotRecordingReason::Unreadable)
    );
    assert_eq!(kpi.enable_setting, None);
    assert!(kpi.spend.is_none());

    // Recording, but nothing attributed to this plan yet: still no invented figure.
    let ledger = std::sync::Arc::new(
        KpiLedger::open_at(&fixture.temp.path().join("kpi_ledger.db")).unwrap(),
    );
    ledger
        .insert(&ledger_event("pl_other", KpiEventKind::Send, None))
        .unwrap();
    fixture.service.kpi = KpiRecorder::with_ledger(ledger);
    let kpi = read_kpi(&fixture);
    assert_eq!(
        kpi.not_recording_reason,
        Some(KpiNotRecordingReason::NoRowsForPlan)
    );
    assert_eq!(kpi.enable_setting, None);
    assert!(kpi.spend.is_none());
}
