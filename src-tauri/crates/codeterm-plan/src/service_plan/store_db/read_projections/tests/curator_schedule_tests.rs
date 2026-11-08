use super::*;
use crate::plan::{CuratorRound, PlanMode, TaskStatus};
use crate::service_plan::curator::{eligible, synthesis};

#[test]
fn scheduler_preserves_complete_compare_value_eligibility_and_group_close() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Schedule".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.mode = PlanMode::Execution;
    plan.team_id = Some("team".into());
    plan.task_groups = serde_json::from_value(json!([{"key":"g","label":"Group"}])).unwrap();
    let mut done = crate::service_plan::load_audit::task("done", TaskStatus::Done, None);
    done.group = Some("g".into());
    done.status_changed_at = 17;
    plan.tasks = vec![
        done,
        crate::service_plan::load_audit::task("open", TaskStatus::Pending, None),
    ];
    plan.strides = serde_json::from_value(json!([
        {"id":"old","from":{"tab_id":"worker"},"kind":"group_close","text":"old","ts":9},
        {"id":"latest","from":{"tab_id":"worker"},"kind":"group_close","text":"large unrelated prose","ts":21},
        {"id":"note","from":{"tab_id":"worker"},"kind":"note","text":"ignore","ts":99}
    ]))
    .unwrap();
    plan.strides.extend(fixtures::round_strides(
        (0..64)
            .map(|number| CuratorRound {
                number,
                ended_at: number,
                ..Default::default()
            })
            .collect(),
    ));
    let database = fixtures::database(directory.path(), &plan);
    let snapshot = database.curator_schedule_projection(&plan.id).unwrap();
    let read = snapshot.metadata();
    assert_eq!(read.control_loop, plan.control_loop);
    assert_eq!(snapshot.group_close(), Some(21));
    assert_eq!(snapshot.group_close(), synthesis::group_close(&plan));
    for enabled in [false, true] {
        for human_wait in [false, true] {
            assert_eq!(
                snapshot.eligible(enabled, human_wait),
                eligible(&plan, enabled, human_wait)
            );
        }
    }
    assert_eq!(
        snapshot.complete(),
        crate::service_plan::heartbeat::is_plan_complete(&plan)
    );
}

#[test]
fn scheduler_materialization_does_not_load_unrelated_history_or_bodies() {
    let mut prior = None;
    for (strides, attempts, extra_body, rounds) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[1].body.push_str(&"x".repeat(extra_body));
        plan.strides.extend(fixtures::round_strides(
            (0..rounds)
                .map(|number| CuratorRound {
                    number,
                    summary: "history".repeat(128),
                    ..Default::default()
                })
                .collect(),
        ));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let snapshot = database.curator_schedule_projection(&plan.id).unwrap();
        let read = snapshot.metadata();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(read.control_loop, plan.control_loop);
        assert!(!snapshot.complete());
        assert_eq!(metrics.queries, 3);
        assert!(metrics.materialized_bytes < 4096);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        let response_bytes = serde_json::to_vec(
            &json!({"eligible":snapshot.eligible(true,false),"group_close":snapshot.group_close()}),
        )
        .unwrap()
        .len();
        println!("fixture={} consumer=curator_schedule tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra_body} rounds={rounds} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn scheduler_snapshot_is_committed_and_decode_failure_is_an_error() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "A".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.tasks = vec![crate::service_plan::load_audit::task(
        "one",
        TaskStatus::Pending,
        None,
    )];
    let database = fixtures::database(directory.path(), &plan);
    let path = directory.path().join("plans.db");
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let id = plan.id.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        for index in 0..16 {
            let mutator = connection.transaction().unwrap();
            let (mode, status) = if index % 2 == 0 {
                ("execution", "done")
            } else {
                ("planning", "pending")
            };
            mutator
                .execute(
                    "UPDATE plans SET header_json=json_set(header_json,'$.mode',?2) WHERE id=?1",
                    params![id, mode],
                )
                .unwrap();
            mutator.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.status',?2) WHERE plan_id=?1",params![id,status]).unwrap();
            writer_barrier.wait();
            mutator.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let snapshot = database.curator_schedule_projection(&plan.id).unwrap();
        let read = snapshot.metadata();
        assert_eq!(read.mode == PlanMode::Execution, snapshot.complete());
        barrier.wait();
    }
    writer.join().unwrap();
    database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.control_loop.mode','invalid') WHERE id=?1",[&plan.id]).unwrap();
    assert!(matches!(
        database.curator_schedule_projection(&plan.id),
        Err(PlanError::Quarantined { .. })
    ));
    assert!(matches!(
        database.curator_schedule_projection("absent"),
        Err(PlanError::NotFound)
    ));
}

#[test]
fn scheduler_exact_cas_rejects_a_concurrent_round_append() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "CAS".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.control_loop.record(CuratorRound {
        number: 1,
        ..Default::default()
    });
    let database = fixtures::database(directory.path(), &plan);
    let snapshot = database.curator_schedule_projection(&plan.id).unwrap();
    let mut next = snapshot.metadata().control_loop.clone();
    next.next_at = Some(42);
    let mut current = plan.control_loop.clone();
    assert!(snapshot
        .apply_control_loop(&mut current, next.clone())
        .unwrap());
    assert_eq!(current.next_at, Some(42));
    let after = current.clone();
    assert!(!snapshot
        .apply_control_loop(&mut current, next.clone())
        .unwrap());
    assert_eq!(current, after);

    // A committed round moves last_round, so the stale snapshot must refuse.
    plan.control_loop.record(CuratorRound {
        number: 2,
        ..Default::default()
    });
    database.persist_plan(&plan, None).unwrap();
    let store = PlanStore::open(directory.path().join("plans.db")).unwrap();
    let applied = store
        .edit_plan_fixture(&plan.id, 99, |latest| {
            snapshot.apply_control_loop(&mut latest.control_loop, next.clone())
        })
        .unwrap();
    assert!(!applied);
    assert_eq!(store.get(&plan.id).unwrap().control_loop, plan.control_loop);
    drop(snapshot);
    let fresh = database.curator_schedule_projection(&plan.id).unwrap();
    let mut next = fresh.metadata().control_loop.clone();
    next.next_at = Some(42);
    let mut current = plan.control_loop.clone();
    assert!(fresh.apply_control_loop(&mut current, next).unwrap());
    assert_eq!(current.last_round, plan.control_loop.last_round);
    assert_eq!(current.next_at, Some(42));
}

#[test]
fn curator_authority_is_selected_coherent_and_loud_on_malformed_run() {
    let mut prior = None;
    for (strides, attempts, rounds) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 2048)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.control_loop.running = Some(crate::plan::CuratorRun {
            number: 7,
            tab_id: Some("curator".into()),
            session_id: Some("session".into()),
            binding_nonce: Some("nonce".into()),
            ..Default::default()
        });
        plan.strides.extend(fixtures::round_strides(
            (0..rounds)
                .map(|number| CuratorRound {
                    number,
                    ..Default::default()
                })
                .collect(),
        ));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let read = database.curator_authority_projection(&plan.id).unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(read.owner, plan.owner);
        assert_eq!(read.running, plan.control_loop.running);
        let response_bytes =
            serde_json::to_vec(&json!({"owner":read.owner,"running":read.running}))
                .unwrap()
                .len();
        assert_eq!(metrics.queries, 1);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=curator_authority tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} rounds={rounds} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0", fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
        database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.control_loop.running.number','invalid') WHERE id=?1", [&plan.id]).unwrap();
        assert!(matches!(
            database.curator_authority_projection(&plan.id),
            Err(PlanError::Quarantined { .. })
        ));
        assert!(matches!(
            database.curator_authority_projection("absent"),
            Err(PlanError::NotFound)
        ));
    }
}

#[test]
fn scheduler_group_aggregate_preserves_terminal_semantics_and_unsigned_timestamps() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Groups".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.task_groups = serde_json::from_value(json!([
        {"key":"done","label":"Done"},{"key":"cancelled","label":"Cancelled"},{"key":"open","label":"Open"}
    ])).unwrap();
    for (id, group, status, at) in [
        ("done", "done", TaskStatus::Done, u64::MAX - 20),
        ("cancel-done", "done", TaskStatus::Cancelled, u64::MAX - 10),
        ("cancelled", "cancelled", TaskStatus::Cancelled, u64::MAX),
        ("open-done", "open", TaskStatus::Done, u64::MAX),
        ("pending", "open", TaskStatus::Pending, 1),
        ("undeclared", "undeclared", TaskStatus::Done, u64::MAX),
    ] {
        let mut task = crate::service_plan::load_audit::task(id, status, None);
        task.group = Some(group.into());
        task.status_changed_at = at;
        plan.tasks.push(task);
    }
    let database = fixtures::database(directory.path(), &plan);
    let read = database.curator_schedule_projection(&plan.id).unwrap();
    assert_eq!(read.group_close(), Some(u64::MAX - 10));
    assert_eq!(read.group_close(), synthesis::group_close(&plan));
    assert!(!read.complete());
    drop(read);
    plan.tasks.clear();
    database.persist_plan(&plan, None).unwrap();
    assert!(!database
        .curator_schedule_projection(&plan.id)
        .unwrap()
        .complete());
    plan.tasks.push(crate::service_plan::load_audit::task(
        "cancelled",
        TaskStatus::Cancelled,
        None,
    ));
    database.persist_plan(&plan, None).unwrap();
    let read = database.curator_schedule_projection(&plan.id).unwrap();
    assert!(read.complete());
    assert_eq!(read.group_close(), None);
    drop(read);
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.status','invalid') WHERE plan_id=?1",[&plan.id]).unwrap();
    assert!(matches!(
        database.curator_schedule_projection(&plan.id),
        Err(PlanError::Quarantined { .. })
    ));
}

#[test]
fn control_loop_display_projects_seven_day_counts_from_round_rows() {
    use crate::plan::{CuratorCounts, CuratorOutcome};
    let now = 30 * 24 * 60 * 60 * 1000_u64;
    let mut previous = None;
    for old_rounds in [0, 2048] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = Plan::new(
            "Control counts".into(),
            directory.path().display().to_string(),
            Default::default(),
        );
        let old = (0..old_rounds).map(|number| CuratorRound {
            number,
            ended_at: 1,
            summary: "expired".repeat(32),
            ..Default::default()
        });
        let recent = (0..20).map(|number| CuratorRound {
            number,
            ended_at: now - 1000,
            outcome: [
                CuratorOutcome::Quiet,
                CuratorOutcome::Nudged,
                CuratorOutcome::Fallback,
                CuratorOutcome::Done,
            ][number as usize % 4],
            learnings: 2,
            save_id: (number % 2 == 0).then(|| format!("save-{number}")),
            ..Default::default()
        });
        plan.strides
            .extend(fixtures::round_strides(old.chain(recent).collect()));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        let read = database.curator_schedule_projection(&plan.id).unwrap();
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let display = read.projected_control_loop(now).unwrap();
        assert_eq!(
            display.counts_7d,
            CuratorCounts {
                rounds: 5,
                quiet: 5,
                nudged: 5,
                learnings: 40,
                saves: 10,
                fallbacks: 5,
            }
        );
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        assert_eq!(metrics.queries, 1);
        assert_eq!(
            crate::service_plan::store_rows::take_serialized_row_bytes(),
            0
        );
        assert!(fixtures::audit_counts(&database).is_empty());
        let measured = (
            metrics,
            decoded,
            serde_json::to_vec(&display).unwrap().len(),
        );
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}
