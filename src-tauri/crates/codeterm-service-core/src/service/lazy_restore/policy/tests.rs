use super::*;
use crate::{
    multiplexer::state::MultiplexerState,
    plan::{OrchestratorRef, TaskExecutionAttempt},
};
use RestoreDecision::{Freeze, Start};

#[test]
fn an_owned_pty_cannot_be_frozen_by_stale_idle_hook_state() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.pty_id = Some("own-live-pty".into());
    tab.agent_state = Some(
        codeterm_types::pty::TabVerdict {
            state: TimelineState::Idle,
            subtype: None,
        }
        .into(),
    );
    assert_eq!(decision(tab, false, Freeze), Start);
    tab.agent_type = None;
    assert_eq!(decision(tab, false, Freeze), Start);
}

#[test]
fn idle_agents_ignore_recent_activity_and_general_workspace_start_decisions() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture".into());
    for shell in [Start, Freeze] {
        assert_eq!(decision(tab, false, shell), Freeze);
        assert_eq!(decision(tab, true, shell), Start);
    }
    tab.protected = true;
    assert_eq!(decision(tab, false, Freeze), Freeze);
    tab.protected = false;
    tab.agent_type = None;
    assert_eq!(decision(tab, false, Start), Start);
    assert_eq!(decision(tab, false, Freeze), Freeze);
}

#[test]
fn active_turns_resume_without_a_plan() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture".into());
    for state in [
        TimelineState::Working,
        TimelineState::Question,
        TimelineState::Idle,
    ] {
        tab.agent_state = Some(
            codeterm_types::pty::TabVerdict {
                state,
                subtype: None,
            }
            .into(),
        );
        assert_eq!(
            decision(tab, false, Freeze),
            if state == TimelineState::Idle {
                Freeze
            } else {
                Start
            }
        );
    }
    tab.agent_runtime_state = Some(serde_json::json!({"active_turn_started_at_ms": 42}));
    assert_eq!(decision(tab, false, Freeze), Start);
    tab.agent_runtime_state = Some(serde_json::json!({"active_turn_started_at_ms": null}));
    assert_eq!(decision(tab, false, Start), Freeze);
}

#[test]
fn snapshot_turn_mark_keeps_interrupted_agent_awake_until_settled() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("own-session".into());
    crate::multiplexer::types::observe_turn(
        &mut tab.turn_resume,
        Some("own-session"),
        "previous-start",
    );
    let mut restored = MultiplexerState::from_session_snapshot(&mux.to_session_snapshot());
    let tab = restored.get_tab_mut(&id).unwrap();
    assert!(tab.agent_state.is_none());
    assert_eq!(decision(tab, false, Freeze), Start);
    crate::multiplexer::types::settle_turn_resume(
        &mut tab.turn_resume,
        "current-start",
        crate::multiplexer::types::TurnResumeOutcome::Sent,
    );
    assert_eq!(decision(tab, false, Start), Freeze);
}

fn plan() -> Plan {
    let mut plan = Plan::new(
        "fixture".into(),
        "fixture".into(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.mode = PlanMode::Execution;
    plan.planner = Some(PlannerRef::Tab {
        tab_id: "planner".into(),
        session_id: None,
        title: None,
    });
    plan.tasks.push(
        serde_json::from_value(serde_json::json!({
            "id": "task", "name": "fixture", "type": "fix", "status": "in_progress"
        }))
        .unwrap(),
    );
    plan.tasks[0].execution_attempts.push(TaskExecutionAttempt {
        id: "attempt".into(),
        executor_tab_id: "worker".into(),
        ..Default::default()
    });
    plan
}

#[test]
fn live_attempt_keeps_worker_owner_and_planner_running() {
    let plan = plan();
    assert_eq!(
        continuing_tabs(&[plan]),
        HashSet::from(["worker".into(), "owner".into(), "planner".into()])
    );
}

#[test]
fn closed_retired_and_nonrunning_work_does_not_wake_tabs() {
    let mut plan = plan();
    plan.tasks[0].execution_attempts[0].closed_at = Some(1);
    assert!(continuing_tabs(&[plan.clone()]).is_empty());
    plan.tasks[0].execution_attempts[0].closed_at = None;
    plan.tasks[0].execution_attempts[0].retired_at = Some(1);
    assert!(continuing_tabs(&[plan.clone()]).is_empty());
    plan.tasks[0].execution_attempts[0].retired_at = None;
    for status in [
        TaskStatus::Pending,
        TaskStatus::InReview,
        TaskStatus::Paused,
        TaskStatus::Done,
    ] {
        plan.tasks[0].status = status;
        assert!(continuing_tabs(&[plan.clone()]).is_empty());
    }
}

#[test]
fn paused_complete_and_archived_plans_do_not_wake_tabs() {
    let mut plan = plan();
    for mode in [PlanMode::Paused, PlanMode::Complete] {
        plan.mode = mode;
        assert!(continuing_tabs(&[plan.clone()]).is_empty());
    }
    plan.mode = PlanMode::Execution;
    plan.status = PlanStatus::Archived;
    assert!(continuing_tabs(&[plan]).is_empty());
}

#[test]
fn remote_attempt_keeps_local_owners_but_not_remote_executor() {
    let mut plan = plan();
    plan.tasks[0].execution_attempts[0].mesh_peer = Some("peer".into());
    assert_eq!(
        continuing_tabs(&[plan]),
        HashSet::from(["owner".into(), "planner".into()])
    );
}
