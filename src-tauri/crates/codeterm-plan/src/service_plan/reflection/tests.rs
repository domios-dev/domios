use super::*;
use crate::plan::{OrchestratorRef, Plan, TaskGroup, TaskStatus};
use crate::service_plan::{NewTask, PlanStore};
use tempfile::TempDir;

fn store_with_group() -> (TempDir, PlanStore, String, Vec<String>) {
    let dir = TempDir::new().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Reflect plan".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    plan.task_groups = vec![TaskGroup {
        key: "wave-1".into(),
        label: "Wave 1".into(),
        color: None,
    }];
    store.save(&mut plan, 1).unwrap();
    let ids = ["a", "b"]
        .iter()
        .map(|n| {
            store
                .add_task(
                    &plan.id,
                    NewTask {
                        name: (*n).into(),
                        kind: "implement".into(),
                        group: Some("wave-1".into()),
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
        .with_plan(pid, now, |p| {
            let t = p.tasks.iter_mut().find(|t| t.id == tid).unwrap();
            t.status = status.clone();
            t.status_changed_at = now;
            Ok(())
        })
        .unwrap();
}

fn learn(store: &PlanStore, pid: &str, text: &str, now: u64) {
    store
        .add_heartbeat_learning(Some(pid), None, None, text, None, now)
        .unwrap();
}

fn close_group(store: &PlanStore, pid: &str, ids: &[String], at: u64) {
    set_status(store, pid, &ids[0], TaskStatus::Done, at - 1);
    set_status(store, pid, &ids[1], TaskStatus::Done, at);
}

#[test]
fn nothing_pending_on_empty_plan() {
    let (_d, store, pid, _ids) = store_with_group();
    let plan = store.get(&pid).unwrap();
    assert!(pending_reflections(&plan, DEFAULT_REFLECT_EVERY_GATES).is_empty());
}

#[test]
fn group_close_fires_once_with_window_counts() {
    let (_d, store, pid, ids) = store_with_group();
    close_group(&store, &pid, &ids, 100);

    let plan = store.get(&pid).unwrap();
    let pending = pending_reflections(&plan, DEFAULT_REFLECT_EVERY_GATES);
    assert_eq!(pending.len(), 1);
    match &pending[0].trigger {
        ReflectionTrigger::GroupClose { key, label } => {
            assert_eq!(key, "wave-1");
            assert_eq!(label, "Wave 1");
        }
        other => panic!("expected group close, got {other:?}"),
    }
    // Both tasks closed since the (zero) baseline; the group's regression gate is
    // still un-run so it registers as an honest gate leak; bounces stay zero.
    assert_eq!(pending[0].window.tasks_closed, 2);
    assert_eq!(pending[0].window.bounces, 0);
    assert_eq!(pending[0].window.gate_leaks, 1);
}

#[test]
fn recording_a_learning_silences_group_reflection() {
    let (_d, store, pid, ids) = store_with_group();
    close_group(&store, &pid, &ids, 100);
    assert_eq!(
        pending_reflections(&store.get(&pid).unwrap(), DEFAULT_REFLECT_EVERY_GATES).len(),
        1
    );

    learn(
        &store,
        &pid,
        "serialize heavy gates in one warm checkout",
        200,
    );
    assert!(
        pending_reflections(&store.get(&pid).unwrap(), DEFAULT_REFLECT_EVERY_GATES).is_empty(),
        "a learning recorded after the close clears the reflection"
    );
}

#[test]
fn reopening_after_learning_rearms_group_reflection() {
    let (_d, store, pid, ids) = store_with_group();
    close_group(&store, &pid, &ids, 100);
    learn(&store, &pid, "first lesson", 200);
    assert!(pending_reflections(&store.get(&pid).unwrap(), DEFAULT_REFLECT_EVERY_GATES).is_empty());

    // Reopen then re-close after the learning: a new close event re-arms.
    set_status(&store, &pid, &ids[1], TaskStatus::InProgress, 300);
    assert!(
        pending_reflections(&store.get(&pid).unwrap(), DEFAULT_REFLECT_EVERY_GATES).is_empty(),
        "an open member means the group is incomplete"
    );
    set_status(&store, &pid, &ids[1], TaskStatus::Done, 400);
    assert_eq!(
        pending_reflections(&store.get(&pid).unwrap(), DEFAULT_REFLECT_EVERY_GATES).len(),
        1,
        "re-closing after the last learning re-arms the reflection"
    );
}

#[test]
fn cadence_fires_at_threshold_and_not_before() {
    let (_d, store, pid, _ids) = store_with_group();
    for i in 0..2 {
        store
            .ask_gate(&pid, &format!("q{i}?"), "", &[], None, None, 10 + i as u64)
            .unwrap();
    }
    // every_gates=3, only 2 events → no cadence reflection.
    assert!(pending_reflections(&store.get(&pid).unwrap(), 3)
        .iter()
        .all(|r| !matches!(r.trigger, ReflectionTrigger::GateCadence { .. })));

    store
        .ask_gate(&pid, "q2?", "", &[], None, None, 30)
        .unwrap();
    let pending = pending_reflections(&store.get(&pid).unwrap(), 3);
    let cadence = pending
        .iter()
        .find(|r| matches!(r.trigger, ReflectionTrigger::GateCadence { .. }))
        .expect("cadence reflection at the 3rd gate");
    match &cadence.trigger {
        ReflectionTrigger::GateCadence { events, every } => {
            assert_eq!(*events, 3);
            assert_eq!(*every, 3);
        }
        _ => unreachable!(),
    }
}

#[test]
fn cadence_silences_then_rearms_on_next_window() {
    let (_d, store, pid, _ids) = store_with_group();
    for i in 0..3 {
        store
            .ask_gate(&pid, &format!("q{i}?"), "", &[], None, None, 10 + i as u64)
            .unwrap();
    }
    assert!(pending_reflections(&store.get(&pid).unwrap(), 3)
        .iter()
        .any(|r| matches!(r.trigger, ReflectionTrigger::GateCadence { .. })));

    learn(&store, &pid, "cadence lesson", 100);
    assert!(
        pending_reflections(&store.get(&pid).unwrap(), 3)
            .iter()
            .all(|r| !matches!(r.trigger, ReflectionTrigger::GateCadence { .. })),
        "learning clears the current cadence window"
    );

    for i in 3..6 {
        store
            .ask_gate(&pid, &format!("q{i}?"), "", &[], None, None, 200 + i as u64)
            .unwrap();
    }
    assert!(
        pending_reflections(&store.get(&pid).unwrap(), 3)
            .iter()
            .any(|r| matches!(r.trigger, ReflectionTrigger::GateCadence { .. })),
        "crossing the next multiple re-arms the cadence reflection"
    );
}

#[test]
fn cadence_disabled_when_every_is_zero() {
    let (_d, store, pid, _ids) = store_with_group();
    for i in 0..5 {
        store
            .ask_gate(&pid, &format!("q{i}?"), "", &[], None, None, 10 + i as u64)
            .unwrap();
    }
    assert!(pending_reflections(&store.get(&pid).unwrap(), 0)
        .iter()
        .all(|r| !matches!(r.trigger, ReflectionTrigger::GateCadence { .. })));
}

#[test]
fn render_emits_block_with_counts_and_routing_primitives() {
    let (_d, store, pid, ids) = store_with_group();
    close_group(&store, &pid, &ids, 100);
    let plan = store.get(&pid).unwrap();

    let mut out = String::new();
    render_reflection(&plan, DEFAULT_REFLECT_EVERY_GATES, &mut out);
    assert!(out.contains(crate::service_plan::prompts_plan::REFLECTION_HEARTBEAT_TITLE));
    assert!(out.contains("tasks closed: 2"));
    assert!(out.contains("gate leaks: 1"));
    assert!(out.contains("codeterm orchestrator learn"));
    assert!(out.contains("codeterm plan skill new"));
    assert!(out.contains("Planner"));
}

#[test]
fn render_is_silent_without_a_trigger() {
    let (_d, store, pid, _ids) = store_with_group();
    let plan = store.get(&pid).unwrap();
    let mut out = String::new();
    render_reflection(&plan, DEFAULT_REFLECT_EVERY_GATES, &mut out);
    assert!(out.is_empty());
}

#[test]
fn bounce_milestones_count_honestly_in_window_and_cadence() {
    let (_d, store, pid, _ids) = store_with_group();
    for i in 0..2 {
        store
            .add_plan_stride(
                &pid,
                BOUNCE_MILESTONE_KIND,
                &format!("bounce {i}"),
                crate::plan::AgentRef::from_tab("orch"),
                10 + i,
            )
            .unwrap();
    }

    let plan = store.get(&pid).unwrap();
    assert_eq!(gate_bounce_event_count(&plan), 2);
    let pending = pending_reflections(&plan, 2);
    let cadence = pending
        .iter()
        .find(|r| matches!(r.trigger, ReflectionTrigger::GateCadence { .. }))
        .expect("bounce milestones should arm cadence");
    assert_eq!(cadence.window.bounces, 2);
}
