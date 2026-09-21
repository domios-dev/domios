use super::*;
use crate::plan::{OrchestratorRef, TaskLaneMetadata};
use crate::service::plan::{NewTask, PlanError, PlanStore, RPC_DISPATCH_IN_FLIGHT};
use std::sync::{Arc, Barrier};

#[test]
fn concurrent_dispatch_of_the_same_task_is_refused_while_one_is_in_flight() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(PlanStore::new_for_test(root.path()));
    let plan = store
        .create_plan_for_team(
            "concurrent dispatch".into(),
            root.path().display().to_string(),
            OrchestratorRef::from_tab("orchestrator"),
            "team".into(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "same task".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let mut runtime = DispatchRuntime::new();
    let (first_tab, first_pty) = runtime.create_worker();
    let (second_tab, second_pty) = runtime.create_worker();
    let barrier = Arc::new(Barrier::new(3));
    let starts = [first_tab.clone(), second_tab.clone()].map(|tab_id| {
        let store = store.clone();
        let barrier = barrier.clone();
        let plan_id = plan.id.clone();
        let task_id = task_id.clone();
        std::thread::spawn(move || {
            barrier.wait();
            let result = store.begin_task_dispatch(
                &plan_id,
                &task_id,
                &tab_id,
                "Worker",
                TaskLaneMetadata::default(),
                3,
            );
            (tab_id, result)
        })
    });
    barrier.wait();

    let outcomes = starts.map(|start| start.join().unwrap());
    let mut winner = None;
    let mut refusal = None;
    for (tab_id, result) in outcomes {
        match result {
            Ok(attempt_id) => {
                assert!(winner
                    .replace((tab_id, attempt_id))
                    .is_none());
            }
            Err(error) => {
                assert_eq!(error.rpc_code(), RPC_DISPATCH_IN_FLIGHT);
                assert_eq!(error.to_json()["type"], "dispatch_in_flight");
                let message = error.to_string();
                let PlanError::DispatchInFlight { attempt_id, tab_id } = error else {
                    panic!("expected dispatch_in_flight error")
                };
                assert!(message.contains(&attempt_id));
                assert!(message.contains(&tab_id));
                assert!(refusal.replace((attempt_id, tab_id)).is_none());
            }
        }
    }
    let (winner_tab, winner_attempt) = winner.expect("exactly one dispatch succeeds");
    let (refused_attempt, refused_tab) = refusal.expect("the other dispatch is refused");
    assert_eq!(refused_attempt, winner_attempt);
    assert_eq!(refused_tab, winner_tab);
    let losing_tab = if winner_tab == first_tab {
        second_tab.clone()
    } else {
        first_tab.clone()
    };
    rollback_failed_dispatch(None, || Ok(()), || runtime.close_worker(&losing_tab)).unwrap();

    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.assigned_tab_id.as_deref(), Some(winner_tab.as_str()));
    assert_eq!(task.execution_attempts.len(), 1);
    assert_eq!(task.execution_attempts[0].id, winner_attempt);
    assert_eq!(task.execution_attempts[0].executor_tab_id, winner_tab);
    assert_eq!(task.execution_attempts[0].closed_at, None);
    assert!(runtime.mux.get_tab(&winner_tab).is_some());
    assert!(runtime.mux.get_tab(&losing_tab).is_none());
    assert_eq!(runtime.tab_count(), runtime.baseline_tab_count + 1);
    let losing_pty = if losing_tab == first_tab {
        first_pty
    } else {
        second_pty
    };
    assert!(runtime.pty.get_session(&losing_pty).is_none());
    let winner_pty = if winner_tab == first_tab {
        first_pty
    } else {
        second_pty
    };
    assert!(runtime.pty.get_session(&winner_pty).is_some());
    runtime.close_worker(&winner_tab).unwrap();
}
