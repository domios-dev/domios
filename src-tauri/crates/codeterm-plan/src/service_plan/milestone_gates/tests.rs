use super::*;
use crate::plan::{OrchestratorRef, TaskGroup, TaskStatus};
use crate::service_plan::NewTask;
use tempfile::TempDir;

fn store_with_two_groups() -> (TempDir, PlanStore, String, Vec<String>) {
    let dir = TempDir::new().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Gate plan".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    plan.task_groups = vec![
        TaskGroup {
            key: "phase-1".into(),
            label: "Phase 1".into(),
            color: None,
        },
        TaskGroup {
            key: "phase-2".into(),
            label: "Phase 2".into(),
            color: None,
        },
    ];
    store.save(&mut plan, 1).unwrap();
    let ids = ["a", "b", "c"]
        .iter()
        .map(|n| {
            store
                .add_task(
                    &plan.id,
                    NewTask {
                        name: (*n).into(),
                        kind: "implement".into(),
                        group: Some(if *n == "c" {
                            "phase-2".into()
                        } else {
                            "phase-1".into()
                        }),
                        ..Default::default()
                    },
                    2,
                )
                .unwrap()
        })
        .collect();
    (dir, store, plan.id, ids)
}

fn set_status(store: &PlanStore, pid: &str, tid: &str, status: TaskStatus, now: u64) {
    store
        .edit_plan_fixture(pid, now, |p| {
            let t = p.tasks.iter_mut().find(|t| t.id == tid).unwrap();
            t.status = status.clone();
            t.status_changed_at = now;
            Ok(())
        })
        .unwrap();
}

#[test]
fn incomplete_group_never_fires() {
    let (_d, store, pid, ids) = store_with_two_groups();
    set_status(&store, &pid, &ids[0], TaskStatus::Done, 10);
    // ids[1] still pending in phase-1.
    let plan = store.get(&pid).unwrap();
    assert!(pending_regression_gates(&plan).is_empty());
}

#[test]
fn last_task_close_fires_once_and_record_silences() {
    let (_d, store, pid, ids) = store_with_two_groups();
    set_status(&store, &pid, &ids[0], TaskStatus::Done, 10);
    set_status(&store, &pid, &ids[1], TaskStatus::Done, 20);

    let plan = store.get(&pid).unwrap();
    let gates = pending_regression_gates(&plan);
    assert_eq!(gates.len(), 1);
    assert_eq!(gates[0].key, "phase-1");
    assert_eq!(gates[0].label, "Phase 1");

    store
        .record_group_regression(
            &pid,
            "phase-1",
            "full suite green",
            AgentRef::from_tab("qa"),
            30,
        )
        .unwrap();
    let plan = store.get(&pid).unwrap();
    assert!(
        pending_regression_gates(&plan).is_empty(),
        "recording the regression run silences the gate"
    );
}

#[test]
fn reopening_a_task_rearms_the_gate() {
    let (_d, store, pid, ids) = store_with_two_groups();
    set_status(&store, &pid, &ids[0], TaskStatus::Done, 10);
    set_status(&store, &pid, &ids[1], TaskStatus::Done, 20);
    store
        .record_group_regression(&pid, "phase-1", "green", AgentRef::from_tab("qa"), 30)
        .unwrap();
    assert!(pending_regression_gates(&store.get(&pid).unwrap()).is_empty());

    // Reopen then re-close a member after the regression run — new work, stale gate.
    set_status(&store, &pid, &ids[1], TaskStatus::InProgress, 40);
    assert!(
        pending_regression_gates(&store.get(&pid).unwrap()).is_empty(),
        "an open member means the group is incomplete, so no gate yet"
    );
    set_status(&store, &pid, &ids[1], TaskStatus::Done, 50);

    let gates = pending_regression_gates(&store.get(&pid).unwrap());
    assert_eq!(
        gates.len(),
        1,
        "reclosing after the old record re-arms the gate"
    );
    assert_eq!(gates[0].key, "phase-1");
}

#[test]
fn all_cancelled_group_has_nothing_to_gate() {
    let (_d, store, pid, ids) = store_with_two_groups();
    set_status(&store, &pid, &ids[0], TaskStatus::Cancelled, 10);
    set_status(&store, &pid, &ids[1], TaskStatus::Cancelled, 20);
    assert!(pending_regression_gates(&store.get(&pid).unwrap()).is_empty());
}

#[test]
fn record_rejects_unknown_group_and_empty_note() {
    let (_d, store, pid, _ids) = store_with_two_groups();
    assert!(matches!(
        store.record_group_regression(&pid, "phase-1", "  ", AgentRef::from_tab("qa"), 5),
        Err(PlanError::QaNoteRequired)
    ));
    assert!(matches!(
        store.record_group_regression(&pid, "nope", "green", AgentRef::from_tab("qa"), 5),
        Err(PlanError::GroupNotFound)
    ));
}

fn close_phase_1(store: &PlanStore, pid: &str, ids: &[String]) {
    set_status(store, pid, &ids[0], TaskStatus::Done, 10);
    set_status(store, pid, &ids[1], TaskStatus::Done, 20);
}

#[test]
fn dispatch_clear_when_no_pending_gates() {
    let (_d, store, pid, ids) = store_with_two_groups();
    set_status(&store, &pid, &ids[0], TaskStatus::Done, 10);
    // phase-1 still has an open member, so nothing gates.
    let plan = store.get(&pid).unwrap();
    assert_eq!(
        regression_dispatch_decision(&plan),
        RegressionDispatchDecision::Clear
    );
}

#[test]
fn dispatch_advisory_names_the_pending_gate() {
    let (_d, store, pid, ids) = store_with_two_groups();
    close_phase_1(&store, &pid, &ids);
    let plan = store.get(&pid).unwrap();
    match regression_dispatch_decision(&plan) {
        RegressionDispatchDecision::Advisory {
            regression_gates, ..
        } => {
            assert_eq!(regression_gates.len(), 1);
            assert_eq!(regression_gates[0].key, "phase-1");
        }
        other => panic!("expected advisory, got {other:?}"),
    }
}

#[test]
fn decision_is_a_pure_advisory_over_plain_gate_inputs() {
    let gates = vec![RegressionGate {
        key: "phase-1".into(),
        label: "Phase 1".into(),
    }];
    let retrospectives = vec![RegressionGate {
        key: "phase-2".into(),
        label: "Phase 2".into(),
    }];

    assert_eq!(
        regression_dispatch_advisory(&gates, &retrospectives),
        RegressionDispatchDecision::Advisory {
            regression_gates: gates,
            retrospectives,
        }
    );
}

#[test]
fn override_records_audit_without_hiding_the_dispatch_advisory() {
    let (_d, store, pid, ids) = store_with_two_groups();
    close_phase_1(&store, &pid, &ids);
    let entry = store
        .record_regression_override(
            &pid,
            vec![],
            "warm QA unavailable; shipping hotfix under owner sign-off",
            AgentRef::from_tab("orch"),
            30,
        )
        .unwrap();
    assert_eq!(entry.gates, vec!["phase-1".to_string()]);
    assert_eq!(entry.ts, 30);

    let plan = store.get(&pid).unwrap();
    assert!(
        blocking_regression_gates(&plan).is_empty(),
        "override drops the gate from the blocking set"
    );
    assert_eq!(
        pending_regression_gates(&plan).len(),
        1,
        "advisory set still shows the gate needs a real QA run"
    );
    assert_eq!(plan.regression_gate_overrides.len(), 1);
    store
        .close_group(&pid, "phase-1", "retro", AgentRef::from_tab("orch"), 31)
        .unwrap();
    match regression_dispatch_decision(&store.get(&pid).unwrap()) {
        RegressionDispatchDecision::Advisory {
            regression_gates, ..
        } => assert_eq!(regression_gates[0].key, "phase-1"),
        other => panic!("expected advisory, got {other:?}"),
    }
}

#[test]
fn override_requires_reason_and_owner() {
    let (_d, store, pid, ids) = store_with_two_groups();
    close_phase_1(&store, &pid, &ids);
    assert!(matches!(
        store.record_regression_override(&pid, vec![], "  ", AgentRef::from_tab("orch"), 30),
        Err(PlanError::OverrideReasonRequired)
    ));
    assert!(matches!(
        store.record_regression_override(
            &pid,
            vec![],
            "sneaky bypass",
            AgentRef::from_tab("worker"),
            30
        ),
        Err(PlanError::OverrideNotAuthorized)
    ));
    // Nothing was recorded by the rejected attempts.
    assert!(store
        .get(&pid)
        .unwrap()
        .regression_gate_overrides
        .is_empty());
}

#[test]
fn override_rearms_when_group_recloses_after_the_waiver() {
    let (_d, store, pid, ids) = store_with_two_groups();
    close_phase_1(&store, &pid, &ids);
    store
        .record_regression_override(&pid, vec![], "owner waiver", AgentRef::from_tab("orch"), 30)
        .unwrap();
    assert!(blocking_regression_gates(&store.get(&pid).unwrap()).is_empty());

    // New work reopens then recloses a member past the waiver — gate re-arms.
    set_status(&store, &pid, &ids[1], TaskStatus::InProgress, 40);
    set_status(&store, &pid, &ids[1], TaskStatus::Done, 50);
    let plan = store.get(&pid).unwrap();
    let blocking = blocking_regression_gates(&plan);
    assert_eq!(blocking.len(), 1, "stale waiver no longer covers new close");
    assert_eq!(blocking[0].key, "phase-1");
}

#[test]
fn clearing_one_group_leaves_the_other_blocking() {
    let (_d, store, pid, ids) = store_with_two_groups();
    close_phase_1(&store, &pid, &ids);
    set_status(&store, &pid, &ids[2], TaskStatus::Done, 25); // phase-2 closes
    let plan = store.get(&pid).unwrap();
    assert_eq!(blocking_regression_gates(&plan).len(), 2);

    store
        .record_group_regression(&pid, "phase-1", "green", AgentRef::from_tab("qa"), 30)
        .unwrap();
    let plan = store.get(&pid).unwrap();
    let blocking = blocking_regression_gates(&plan);
    assert_eq!(blocking.len(), 1, "phase-2 still gates dispatch");
    assert_eq!(blocking[0].key, "phase-2");
}

fn retrospective_keys(store: &PlanStore, pid: &str) -> Vec<String> {
    pending_group_retrospectives(&store.get(pid).unwrap())
        .into_iter()
        .map(|gate| gate.key)
        .collect()
}

fn record_retrospective(store: &PlanStore, pid: &str, now: u64) {
    store
        .close_group(pid, "phase-1", "retro", AgentRef::from_tab("orch"), now)
        .unwrap();
}

#[test]
fn milestone_retrospective_contract_terminal_group_gates_next_frontier() {
    let (_d, store, pid, ids) = store_with_two_groups();
    close_phase_1(&store, &pid, &ids);
    store
        .record_group_regression(&pid, "phase-1", "green", AgentRef::from_tab("qa"), 30)
        .unwrap();
    let plan = store.get(&pid).unwrap();
    assert!(blocking_regression_gates(&plan).is_empty());
    match regression_dispatch_decision(&plan) {
        RegressionDispatchDecision::Advisory {
            regression_gates,
            retrospectives,
        } => {
            assert!(regression_gates.is_empty());
            assert_eq!(retrospectives.len(), 1);
            assert_eq!(retrospectives[0].key, "phase-1");
        }
        other => panic!("expected retrospective advisory, got {other:?}"),
    }

    record_retrospective(&store, &pid, 40);
    let plan = store.get(&pid).unwrap();
    assert!(retrospective_keys(&store, &pid).is_empty());
    let retro = plan
        .strides
        .iter()
        .find(|m| m.kind == GROUP_CLOSE_MILESTONE_KIND)
        .unwrap();
    assert_eq!(retro.links, vec![regression_link("phase-1")]);
    assert_eq!(
        regression_dispatch_decision(&plan),
        RegressionDispatchDecision::Clear
    );
}

#[test]
fn milestone_retrospective_contract_reclose_rearms_the_retrospective() {
    let (_d, store, pid, ids) = store_with_two_groups();
    close_phase_1(&store, &pid, &ids);
    record_retrospective(&store, &pid, 30);
    assert!(retrospective_keys(&store, &pid).is_empty());

    set_status(&store, &pid, &ids[1], TaskStatus::InProgress, 40);
    assert!(
        retrospective_keys(&store, &pid).is_empty(),
        "an open member means the group is not terminal"
    );
    set_status(&store, &pid, &ids[1], TaskStatus::Done, 50);
    assert_eq!(
        retrospective_keys(&store, &pid),
        vec!["phase-1".to_string()]
    );
}

#[test]
fn milestone_retrospective_contract_early_close_does_not_cover_later_completion() {
    let (_d, store, pid, ids) = store_with_two_groups();
    record_retrospective(&store, &pid, 5);
    assert!(retrospective_keys(&store, &pid).is_empty());
    close_phase_1(&store, &pid, &ids);
    assert_eq!(
        retrospective_keys(&store, &pid),
        vec!["phase-1".to_string()]
    );
}

#[test]
fn milestone_retrospective_contract_override_never_waives_it() {
    let (_d, store, pid, ids) = store_with_two_groups();
    close_phase_1(&store, &pid, &ids);
    store
        .record_regression_override(&pid, vec![], "waiver", AgentRef::from_tab("orch"), 30)
        .unwrap();
    let plan = store.get(&pid).unwrap();
    assert!(blocking_regression_gates(&plan).is_empty());
    assert!(matches!(
        regression_dispatch_decision(&plan),
        RegressionDispatchDecision::Advisory { ref retrospectives, .. } if retrospectives.len() == 1
    ));
}

#[test]
fn milestone_retrospective_contract_skips_open_and_all_cancelled_groups() {
    let (_d, store, pid, ids) = store_with_two_groups();
    set_status(&store, &pid, &ids[0], TaskStatus::Done, 10);
    assert!(retrospective_keys(&store, &pid).is_empty());
    set_status(&store, &pid, &ids[0], TaskStatus::Cancelled, 11);
    set_status(&store, &pid, &ids[1], TaskStatus::Cancelled, 12);
    assert!(retrospective_keys(&store, &pid).is_empty());
}
