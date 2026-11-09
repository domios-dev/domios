use super::*;
use crate::plan::{Milestone, PlanMode, TaskExecutionAttempt, TaskStatus};
use crate::service_plan::digest_rules::plan_digest_counters;

fn milestone(id: &str, kind: &str, ts: u64) -> Milestone {
    serde_json::from_value(
        json!({"id":id,"from":{"tab_id":"owner"},"kind":kind,"text":"fixed","ts":ts}),
    )
    .unwrap()
}

#[test]
fn notice_cleanup_matches_full_history_dedup_safety_and_candidate_order() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Notices".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    for (index, status, peer, path, cleanup, hints) in [
        (0, TaskStatus::Done, None, " /shared ", None, vec![]),
        (
            1,
            TaskStatus::Cancelled,
            None,
            "/shared",
            None,
            vec!["DIRTY".into()],
        ),
        (
            2,
            TaskStatus::Done,
            Some("peer"),
            "\u{2003}/remote\u{2003}",
            Some("pending"),
            vec!["UnMeRgEd branch".into()],
        ),
        (
            3,
            TaskStatus::Done,
            None,
            "/cleaned",
            Some("ClEaNeD"),
            vec![],
        ),
        (4, TaskStatus::InProgress, None, "/active", None, vec![]),
        (
            5,
            TaskStatus::Done,
            Some(""),
            "/shared",
            Some("debt"),
            vec![],
        ),
    ] {
        let mut task =
            crate::service_plan::load_audit::task(&format!("task_{index}"), status, None);
        task.peer = peer.map(str::to_string);
        task.lane.worktree_path = Some(path.into());
        task.lane.cleanup_status = cleanup.map(str::to_string);
        task.lane.cleanup_hints = hints;
        task.execution_attempts = vec![TaskExecutionAttempt {
            id: format!("old_{index}"),
            closed_at: Some(1),
            worktree_path: Some("/local-attempt".into()),
            remote_worktree_path: Some("/remote-attempt".into()),
            mesh_peer: Some("peer".into()),
            ..Default::default()
        }];
        plan.tasks.push(task);
    }
    for index in 6..20 {
        let mut task =
            crate::service_plan::load_audit::task(&format!("task_{index}"), TaskStatus::Done, None);
        task.lane.worktree_path = Some(format!("/lane-{index}"));
        if index == 19 {
            task.lane.cleanup_hints = vec!["dirty".into()];
        }
        plan.tasks.push(task);
    }
    plan.strides = vec![milestone("map", "map_requested", 12)];
    plan.tasks[0]
        .milestones
        .push(milestone("task-map", "map_generated", 18));
    plan.strides
        .extend((0..40).map(|i| milestone(&format!("anchor{i}"), "note", 100 + i)));
    let database = fixtures::database(directory.path(), &plan);
    let read = database.notice_projection(&plan.id).unwrap();
    let input = read.digest_input(10_000, &Default::default()).unwrap();
    let actual = input.queue_counters();
    assert_eq!(actual, plan_digest_counters(&plan, 10_000, 0));
    assert_eq!(actual.finished_lanes, 18);
    assert_eq!(actual.lane_candidates.len(), 5);
    assert!(actual.lane_candidates[0].contains("retain dirty/unmerged"));
    assert_eq!(input.cartography.latest_map_at, 18);
}

#[test]
fn notice_materialization_and_heartbeat_ignore_unrelated_history_and_bodies() {
    let mut prior = None;
    for (strides, attempts, extra_body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.mode = PlanMode::Execution;
        plan.tasks[1].body.push_str(&"x".repeat(extra_body));
        plan.tasks[1].status = TaskStatus::Done;
        plan.tasks[1].lane.worktree_path = Some("/finished".into());
        for attempt in &mut plan.tasks[1].execution_attempts {
            attempt.worktree_path = Some("/finished".into());
        }
        plan.strides
            .extend((0..256).map(|i| milestone(&format!("anchor-{i:04}"), "note", 1000 + i)));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        super::super::super::super::store_sync::take_decoded_row_bytes();
        let full = database.load_plan(&plan.id).unwrap().unwrap();
        let baseline_decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        take_metrics();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let read = database.notice_projection(&plan.id).unwrap();
        let counters = read
            .digest_input(10_000, &Default::default())
            .unwrap()
            .queue_counters();
        assert_eq!(counters, plan_digest_counters(&full, 10_000, 0));
        let digests = vec![format!("finished={}", counters.finished_lanes)];
        let response = read
            .prepend_heartbeat(10_000, "fixed", &digests, None)
            .unwrap();
        assert_eq!(
            response,
            crate::service_plan::heartbeat::prepend_plan_heartbeat_with_digests(
                &full, "fixed", &digests
            )
        );
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        assert!(metrics.materialized_bytes < response.len() + 16 * 1024);
        assert!(decoded < 16 * 1024);
        println!("fixture={} consumer=notice tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra_body} anchor_strides=256 baseline_decoded_json_bytes={baseline_decoded} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes,response.len());
        let measured = (metrics, decoded, response.len());
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn notice_header_and_cleanup_read_one_committed_snapshot_and_fail_loudly() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "A".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.owner.tab_id = Some("A".into());
    let mut task = crate::service_plan::load_audit::task("one", TaskStatus::Done, None);
    task.lane.worktree_path = Some("/lane".into());
    task.lane.cleanup_status = Some("done".into());
    plan.tasks = vec![task];
    let database = fixtures::database(directory.path(), &plan);
    let path = database.path();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let id = plan.id.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        for index in 0..16 {
            let (title, cleanup) = if index % 2 == 0 {
                ("B", "pending")
            } else {
                ("A", "done")
            };
            let tx = connection.transaction().unwrap();
            tx.execute(
                "UPDATE plans SET header_json=json_set(header_json,'$.owner.tab_id',?2) WHERE id=?1",
                params![id, title],
            )
            .unwrap();
            tx.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.lane.cleanup_status',?2) WHERE plan_id=?1",params![id,cleanup]).unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let read = database.notice_projection(&plan.id).unwrap();
        assert_eq!(
            read.owner_tab_id.as_deref() == Some("B"),
            read.digest_input(10_000, &Default::default())
                .unwrap()
                .queue
                .finished_lanes
                == 1
        );
        barrier.wait();
    }
    writer.join().unwrap();
    database
        .connection
        .lock()
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.owner.tab_id',17) WHERE id=?1",
            [&plan.id],
        )
        .unwrap();
    assert!(matches!(
        database.notice_projection(&plan.id),
        Err(PlanError::Quarantined { .. })
    ));
    assert!(matches!(
        database.notice_projection("absent"),
        Err(PlanError::NotFound)
    ));
}
