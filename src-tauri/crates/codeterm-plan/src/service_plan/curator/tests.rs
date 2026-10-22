use super::*;
use crate::plan::{curator_delay, OrchestratorRef, CURATOR_IDLE_FLOOR_MS};

#[test]
fn manual_round_requests_preserve_suspension_and_single_round_gates() {
    let mut state = ControlLoop::default();
    state.next_at = Some(50_000);
    assert!(request_round(&mut state, false, 10).is_err());
    assert_eq!(state.next_at, Some(50_000));
    assert!(!state.manual_run_requested);
    assert!(request_round(&mut state, true, 10).is_ok());
    assert_eq!(state.next_at, Some(10));
    assert!(state.manual_run_requested);
    assert!(request_round(&mut state, true, 11).is_err());
    state.manual_run_requested = false;
    state.mode = ControlLoopMode::Heartbeat;
    assert!(request_round(&mut state, true, 11).is_err());
    state.mode = ControlLoopMode::Curator;
    state.running = Some(Default::default());
    assert!(request_round(&mut state, true, 11).is_err());
}

#[test]
fn curator_cadence_clamps_and_rearms_from_round_end() {
    for (chosen, expected) in [
        (None, 2700),
        (Some(0), 2700),
        (Some(3600), 3600),
        (Some(u64::MAX), 7200),
    ] {
        assert_eq!(curator_delay(chosen), expected);
    }
    let mut plan = Plan::new("test".into(), "/tmp".into(), OrchestratorRef::default());
    append_round(
        &mut plan,
        CuratorRound {
            ended_at: 50_000,
            next_delay_secs: 500,
            ..Default::default()
        },
    );
    assert_eq!(plan.control_loop.next_at, Some(2_750_000));
    plan.control_loop.mode = ControlLoopMode::Heartbeat;
    append_round(
        &mut plan,
        CuratorRound {
            ended_at: 60_000,
            next_delay_secs: 7200,
            ..Default::default()
        },
    );
    assert_eq!(plan.control_loop.next_at, None);
}

#[test]
fn curator_pre_gate_has_three_cases_and_an_idle_floor() {
    assert_eq!(due_action(true, false, Some(0), 10), DueAction::Spawn);
    assert_eq!(due_action(false, true, Some(0), 10), DueAction::Nudge);
    assert_eq!(due_action(false, false, Some(0), 10), DueAction::Quiet);
    assert_eq!(
        due_action(false, false, Some(0), CURATOR_IDLE_FLOOR_MS),
        DueAction::Spawn
    );
}

#[test]
fn curator_human_wait_and_lifecycle_override_idle_floor() {
    let mut plan = Plan::new("test".into(), "/tmp".into(), Default::default());
    plan.team_id = Some("team".into());
    plan.mode = PlanMode::Execution;
    assert!(eligible(&plan, true, false));
    assert!(!eligible(&plan, true, true));
    assert!(!eligible(&plan, false, false));
    plan.mode = PlanMode::Paused;
    assert!(!eligible(&plan, true, false));
    plan.mode = PlanMode::Complete;
    assert!(!eligible(&plan, true, false));
}

#[test]
fn curator_producer_preserves_owner_mode_and_fallback_default_interval() {
    let mut state = ControlLoop::default();
    assert!(!state.owner_heartbeat());
    state.fallback_reason = Some(codeterm_types::plan::CuratorFailure::MirrorUnavailable);
    state.heartbeat_interval_secs = Some(1800);
    assert!(state.owner_heartbeat());
    assert_eq!(state.interval(3600), 3600);
    state.mode = ControlLoopMode::Heartbeat;
    assert!(state.owner_heartbeat());
    assert_eq!(state.interval(3600), 1800);
}

#[test]
fn curator_digest_is_bounded_stable_and_does_not_treat_waiting_as_activity() {
    let row = |n| digest::Row {
        id: format!("task_{n:03}"),
        kind: "task".into(),
        state: "done".into(),
        summary: "é".repeat(400),
    };
    let mut sections = BTreeMap::new();
    for (name, _) in digest::SECTIONS {
        sections.insert(name.into(), (0..40).rev().map(row).collect());
    }
    let first = digest::build("header", &sections);
    assert!(first.body.len() <= digest::MAX_DIGEST_BYTES);
    assert!(first.body.contains("\ndecisions:\n"));
    assert!(first.body.contains("+30 more"));
    for rows in sections.values_mut() {
        rows.reverse();
    }
    assert_eq!(first.body, digest::build("header", &sections).body);
    let only_waiting = BTreeMap::from([("actionable".into(), vec![row(1)])]);
    assert!(!digest::build("header", &only_waiting).activity);
}

#[test]
fn curator_note_validates_contract_and_drops_ungrounded_learnings() {
    let input="NEXT: 2h | quiet window\nMOVE: inspect [task_1]\nRETRO:\n+ useful [task_1]\n- no evidence\nLEARN: when retry fails -> inspect cause [task_1 task_2] (confidence low)\nNEXT-CHECK: verify result [task_1]\nASKED: none";
    let parsed = note::parse(input, 7200).unwrap();
    assert_eq!(parsed.retro.len(), 1);
    assert_eq!(parsed.learnings.len(), 1);
    assert_eq!(parsed.next_checks.len(), 1);
    assert!(note::parse(input, 2700).is_err());
    assert!(note::parse("NEXT: 2h | reason\nASKED: yes", 7200).is_err());
    assert!(note::parse(
        &format!(
            "NEXT: 2h | reason\nMOVE: inspect [task_1]\nRETRO:\n{}\nASKED: none",
            (0..6)
                .map(|n| format!("+ fact [task_{n}]"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
        7200
    )
    .is_err());
}

#[test]
fn curator_movement_orders_oldest_tasks_without_creating_activity() {
    let mut plan = Plan::new("fixture".into(), "/tmp".into(), Default::default());
    for index in 0..14 {
        let mut task: crate::plan::Task = serde_json::from_value(serde_json::json!({"id":format!("task_{index:02}"),"name":"opaque","type":"code","status":"in_review"})).unwrap();
        task.status_changed_at = 100 + index;
        plan.tasks.push(task);
    }
    let rows = movement::rows(&plan, 0, 1000);
    assert_eq!(rows["review backlog"][0].state, "14");
    assert_eq!(rows["review backlog"][0].summary, "900");
    let digest = digest::build("header", &rows);
    assert!(!digest.activity);
    assert!(
        digest.body.find("task_00 | task").unwrap() < digest.body.find("task_09 | task").unwrap()
    );
    assert!(!digest.body.contains("task_10 | task"));
    assert!(digest.body.contains("+4 more"));
    let mut rows = rows;
    rows.insert(
        "reports".into(),
        vec![digest::Row {
            id: "report_1".into(),
            kind: "report".into(),
            state: "recorded".into(),
            summary: String::new(),
        }],
    );
    assert!(digest::build_bounded(&"x".repeat(100), &rows, 32).activity);
}

#[test]
fn curator_move_requires_cited_ids() {
    assert!(note::parse("NEXT: 45m | reason\nASKED: none", 2700).is_err());
    assert!(note::parse("NEXT: 45m | reason\nMOVE: inspect\nASKED: none", 2700).is_err());
    let note = note::parse(
        "NEXT: 45m | reason\nMOVE: inspect [task_1]\nASKED: none",
        2700,
    )
    .unwrap();
    assert_eq!(note.next_move, "inspect [task_1]");
}
