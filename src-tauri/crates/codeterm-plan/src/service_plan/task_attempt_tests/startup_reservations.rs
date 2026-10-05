use super::*;

#[test]
fn concurrent_startup_reservations_refuse_before_worker_spawn() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    let store = std::sync::Arc::new(store);
    let observed = std::sync::Arc::new(std::sync::Barrier::new(2));
    let reserved = std::sync::Arc::new(std::sync::Barrier::new(2));
    let attempts =
        [("attempt-a", "worker-a"), ("attempt-b", "worker-b")].map(|(attempt_id, tab_id)| {
            let store = store.clone();
            let plan_id = plan_id.clone();
            let task_id = task_id.clone();
            let observed = observed.clone();
            let reserved = reserved.clone();
            std::thread::spawn(move || {
                let owner = store.dispatch_startup_owner_candidate(&plan_id, &task_id);
                assert!(owner.is_none());
                observed.wait();
                let result = store.reserve_task_dispatch_startup(
                    &plan_id, &task_id, attempt_id, tab_id, owner, false,
                );
                match result {
                    Ok(reservation) => {
                        reserved.wait();
                        drop(reservation);
                        Ok((attempt_id.to_string(), tab_id.to_string()))
                    }
                    Err(error) => {
                        reserved.wait();
                        Err(error)
                    }
                }
            })
        });

    let outcomes = attempts.map(|attempt| attempt.join().unwrap());
    let (winner_attempt, winner_tab) = outcomes
        .iter()
        .find_map(|outcome| outcome.as_ref().ok().cloned())
        .expect("one startup reservation succeeds");
    let refusal = outcomes
        .into_iter()
        .find_map(Result::err)
        .expect("the competing startup reservation is refused");
    let PlanError::DispatchInFlight { attempt_id, tab_id } = refusal else {
        panic!("expected typed startup reservation refusal")
    };
    assert_eq!(attempt_id, winner_attempt);
    assert_eq!(tab_id, winner_tab);

    let reservation = store
        .reserve_task_dispatch_startup(
            &plan_id,
            &task_id,
            "attempt-after-release",
            "worker-after-release",
            None,
            false,
        )
        .expect("dropping the reservation releases the task");
    drop(reservation);
}

#[test]
fn repeated_runtime_binding_keeps_one_plan_attempt() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    let attempt_id = store
        .begin_task_dispatch(
            &plan_id,
            &task_id,
            "worker-tab",
            "Worker",
            TaskLaneMetadata::default(),
            3,
        )
        .unwrap();
    let attempts_before = store
        .show_task(&task_id)
        .unwrap()
        .task
        .execution_attempts
        .len();

    for now_ms in [4, 5] {
        let ready = store
            .mark_live_task_attempt_runtime_ready_for_tab("worker-tab", "session", now_ms)
            .unwrap()
            .expect("the same live attempt remains bindable");
        assert_eq!(ready.attempt.id, attempt_id);
    }

    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.execution_attempts.len(), attempts_before);
    let bound: Vec<_> = task
        .execution_attempts
        .iter()
        .filter(|attempt| attempt.id == attempt_id)
        .collect();
    assert_eq!(bound.len(), 1);
    assert_eq!(bound[0].session_id.as_deref(), Some("session"));
}

#[test]
fn a_closed_attempt_without_a_newer_live_dispatch_is_not_reported_as_superseded() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    let attempt_id = store
        .begin_task_dispatch(
            &plan_id,
            &task_id,
            "worker-tab",
            "Worker",
            TaskLaneMetadata::default(),
            3,
        )
        .unwrap();
    assert!(store
        .mark_worker_startup_deadline(&plan_id, &task_id, "worker-tab", &attempt_id, true, 4)
        .unwrap());

    let refusal = store
        .acknowledge_live_task_attempt_for_tab("worker-tab", Some(&plan_id), Some(&task_id), 5)
        .expect_err("a closed attempt without a replacement is not acknowledgeable");
    let message = refusal.to_string();
    assert!(message.contains(super::super::task_dispatch::ACK_CAUSE_NO_LIVE_ATTEMPT));
    assert!(!message.contains(super::super::task_dispatch::ACK_CAUSE_ATTEMPT_SUPERSEDED));
}
