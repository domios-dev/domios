use super::super::tests::fixtures::{audit, audit_counts, database, fixture};
use super::*;
use crate::plan::{HeartbeatLearning, OrchestratorRef, Plan};
use std::sync::{Arc, Barrier};

#[test]
fn runtime_header_and_heartbeat_work_is_independent_of_unrelated_history() {
    let mut previous = None;
    for (strides, attempts) in [(0, 0), (2048, 0), (0, 2048)] {
        let dir = tempfile::tempdir().unwrap();
        let plan = fixture(dir.path(), strides, attempts);
        let db = database(dir.path(), &plan);
        audit(&db);
        let (_, header) = db
            .edit_runtime_rows::<PlannerSessionRows, _>(&plan.id, 20, |rows| {
                rows.planner = Some(PlannerRef::Tab {
                    tab_id: "planner".into(),
                    session_id: Some("session".into()),
                    title: None,
                });
                Ok(())
            })
            .unwrap();
        let (_, heartbeat) = db
            .edit_runtime_rows::<HeartbeatRows, _>(&plan.id, 21, |rows| {
                rows.heartbeat.learnings.push(HeartbeatLearning {
                    id: "learn".into(),
                    text: "bounded".into(),
                    ts: 21,
                    tag: None,
                });
                Ok(())
            })
            .unwrap();
        assert_eq!(audit_counts(&db), vec![("plans".into(), 3)]);
        let measured = [header, heartbeat].map(|r| {
            (
                r.task_rows,
                r.attempts,
                r.strides,
                r.header_rows,
                r.decoded_bytes,
                r.serialized_bytes,
            )
        });
        assert!(measured.iter().all(|r| r.0 == 0
            && r.1 == 0
            && r.2 == 0
            && r.4 < 4096
            && r.5 < 4096));
        if let Some(previous) = previous {
            assert_eq!(previous, measured);
        }
        previous = Some(measured);
    }
}

#[test]
fn runtime_stride_and_header_roll_back_together_on_write_failure() {
    let dir = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "pause".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pause".into();
    let db = database(dir.path(), &plan);
    let before = db.load_plan(&plan.id).unwrap().unwrap();
    db.connection.lock().execute_batch("CREATE TRIGGER refuse_header BEFORE UPDATE ON plans BEGIN SELECT RAISE(ABORT, 'injected header failure'); END;").unwrap();
    assert!(db
        .edit_runtime_rows::<PauseRows, _>(&plan.id, 20, |rows| {
            rows.mode = PlanMode::Paused;
            rows.strides.push(crate::plan::Milestone {
                id: "pause-stride".into(),
                from: Default::default(),
                kind: "decision".into(),
                text: "pause".into(),
                ts: 20,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            });
            Ok(())
        })
        .is_err());
    assert_eq!(db.load_plan(&plan.id).unwrap().unwrap(), before);
}

#[test]
fn concurrent_pause_claims_and_planner_binding_never_load_whole_plans() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(PlanStore::new_for_test(dir.path()));
    let mut plan = Plan::new(
        "pause".into(),
        dir.path().display().to_string(),
        OrchestratorRef::default(),
    );
    store.save(&mut plan, 1).unwrap();
    store.clear_plan_cache_for_test();
    store.reset_plan_loads_for_test();
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let store = store.clone();
            let barrier = barrier.clone();
            let id = plan.id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .begin_plan_pause_operation(
                        &id,
                        "pause",
                        Default::default(),
                        vec![],
                        vec![],
                        20,
                    )
                    .unwrap()
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                crate::service_plan::pause_operation::PlanPauseBegin::Applied { .. }
            ))
            .count(),
        1
    );
    assert!(store.plan_loads_for_test().is_empty());
    store
        .with_runtime_rows::<PlannerSessionRows, _>(&plan.id, 21, |rows| {
            rows.planner = Some(PlannerRef::Tab {
                tab_id: "planner".into(),
                session_id: None,
                title: None,
            });
            Ok(())
        })
        .unwrap();
    store
        .complete_planner_report_session(&plan.id, "planner", "session", 22)
        .unwrap();
    assert!(store
        .complete_planner_report_session(&plan.id, "planner", "foreign", 23)
        .is_err());
    assert!(store.plan_loads_for_test().is_empty());
}
