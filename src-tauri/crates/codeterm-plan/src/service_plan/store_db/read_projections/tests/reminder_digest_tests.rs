use super::*;
use crate::plan::TaskStatus;
use crate::service_plan::{digest_rules::plan_digest_counters, NoticeInventoryTargets};

#[test]
fn reminder_digest_facts_match_reference_with_history_and_body_independent_bytes() {
    let mut baseline = None;
    for (history, attempts, body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), history, attempts);
        plan.tasks[0].status = TaskStatus::InReview;
        plan.tasks[0].status_changed_at = 10;
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].status = TaskStatus::InProgress;
        plan.tasks[1].status_changed_at = 1;
        plan.tasks[2].status = TaskStatus::Done;
        plan.tasks[2].status_changed_at = 15;
        plan.tasks[2].group = Some("closed".into());
        plan.tasks[2].assigned_tab_id = Some("finished".into());
        plan.tasks[2].lane.worktree_path = Some("/lane".into());
        plan.task_groups =
            serde_json::from_value(json!([{"key":"closed","label":"Closed wave","color":null}]))
                .unwrap();
        let now = 10_000_000;
        let stalled = crate::plan::reconcile::aged_open_tasks(&plan, now)
            .into_iter()
            .filter(|task| task.status == TaskStatus::InProgress)
            .count();
        let expected = plan_digest_counters(&plan, now, stalled);
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let snapshot = database
            .reminder_read_snapshot(|text| text.chars().take(512).collect())
            .unwrap();
        let read = snapshot.composition(&plan.id).unwrap().unwrap();
        let input = read
            .digest_input(
                now,
                &NoticeInventoryTargets {
                    task_ids: vec![plan.tasks[2].id.clone()],
                    tab_ids: vec!["finished".into()],
                },
            )
            .unwrap();
        assert_eq!(input.queue.in_review, expected.in_review);
        assert_eq!(input.queue.in_progress, expected.in_progress);
        assert_eq!(
            input.queue.oldest_in_review_age_ms,
            expected.oldest_in_review_age_ms
        );
        assert_eq!(input.queue.stalled_lanes, expected.stalled_lanes);
        assert_eq!(input.queue.finished_lanes, expected.finished_lanes);
        assert_eq!(input.queue.lane_candidates, expected.lane_candidates);
        assert_eq!(input.cartography.latest_map_at, 0);
        assert_eq!(input.cartography.release_at, None);
        assert_eq!(
            input.cartography.closed_group.as_ref().unwrap().closed_at,
            15
        );
        assert_eq!(input.finished_bindings.len(), 1);
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&input).unwrap().len();
        assert_eq!(serialized, 0);
        assert!(metrics.materialized_bytes <= response + 4096);
        assert!(decoded <= response + 4096);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumers=reminder_digest_queue,reminder_digest_cartography,reminder_digest_inventory tasks={} old_strides={history} unrelated_attempts={attempts} extra_body={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &baseline {
            assert_eq!(&measured, previous);
        }
        baseline = Some(measured);
    }
}

#[test]
fn reminder_digest_unsigned_times_group_order_and_exact_project_root_are_preserved() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 1);
    plan.canonical_project_path = " ".into();
    plan.tasks[0].status = TaskStatus::InReview;
    plan.tasks[0].status_changed_at = u64::MAX - 7;
    plan.tasks[1].status = TaskStatus::InProgress;
    plan.tasks[1].status_changed_at = u64::MAX;
    for (position, key) in [(2, "a"), (3, "z")] {
        plan.tasks[position].status = TaskStatus::Done;
        plan.tasks[position].status_changed_at = u64::MAX - 1;
        plan.tasks[position].group = Some(key.into());
    }
    let database = fixtures::database(directory.path(), &plan);
    let snapshot = database.reminder_read_snapshot(str::to_string).unwrap();
    let read = snapshot.composition(&plan.id).unwrap().unwrap();
    let input = read.digest_input(u64::MAX, &Default::default()).unwrap();
    assert_eq!(input.project_root, " ");
    assert_eq!(input.queue.oldest_in_review_age_ms, 7);
    assert_eq!(input.queue.stalled_lanes, 0);
    assert_eq!(input.cartography.closed_group.as_ref().unwrap().key, "z");
    assert_eq!(
        input.cartography.closed_group.as_ref().unwrap().closed_at,
        u64::MAX - 1
    );
    drop(read);
    drop(snapshot);
    for task in &mut plan.tasks {
        task.status = TaskStatus::Done;
    }
    database.persist_plan_rows(&plan, None).unwrap();
    let snapshot = database.reminder_read_snapshot(str::to_string).unwrap();
    let read = snapshot.composition(&plan.id).unwrap().unwrap();
    assert_eq!(
        read.digest_input(u64::MAX, &Default::default())
            .unwrap()
            .cartography
            .release_at,
        Some(u64::MAX)
    );
}
