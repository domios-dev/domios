use super::*;
use crate::plan::{Milestone, PlanStatus, PlannerRef, Task, TaskStatus};
use crate::service_plan::{
    planner_replay::planner_replay_candidate,
    task_attempts::{attempt_view, build_prompt, task_report_command},
};

fn milestone(id: &str, kind: &str, text: &str, ts: u64, tab: &str) -> Milestone {
    serde_json::from_value(json!({"id":id,"from":{"tab_id":tab},"kind":kind,"text":text,"ts":ts}))
        .unwrap()
}

fn decorate(plan: &mut Plan) {
    plan.planner = Some(PlannerRef::Tab {
        tab_id: "planner".into(),
        session_id: Some("session".into()),
        title: None,
    });
    plan.planner_requests = serde_json::from_value(
        json!([{"request_id":"request","payload":"opaque","state":"queued","created_at":1}]),
    )
    .unwrap();
    let task = &mut plan.tasks[0];
    task.handle = Some("target".into());
    task.key = Some("KEY-1".into());
    task.lane.workspace_id = Some("workspace".into());
    task.role_guidance = Some("Selected guidance".into());
    task.executor_role = Some("selected-role".into());
    task.personality = Some("Selected identity".into());
    task.intent = Some("Selected intent".into());
    task.acceptance_criteria = vec!["Selected criterion".into()];
    task.correctness = Some("Selected correctness".into());
    task.test_plan = Some("Selected proof".into());
    let mut attempt = crate::service_plan::load_audit::attempt("current", "worker");
    attempt.provider = Some("test-provider".into());
    attempt.role = Some("worker".into());
    attempt.executor_title = Some("Current".into());
    attempt.assigned_at = Some(10);
    attempt.session_id = Some("session".into());
    task.execution_attempts = vec![attempt];
    task.milestones = (0..6)
        .map(|i| {
            milestone(
                &format!("decision-{i}"),
                "decision",
                &format!("Selected decision {i}"),
                i + 1,
                "worker",
            )
        })
        .collect();
    task.milestones.push(milestone(
        "old-report",
        "status_update",
        "STATUS: blocked\nold report",
        u64::MAX,
        "worker",
    ));
    task.milestones.push(milestone(
        "latest-report",
        "status_update",
        "STATUS: in_review\nlatest report",
        u64::MAX,
        "worker",
    ));
    task.milestones.push(milestone(
        "other-note",
        "note",
        "Other executor note",
        u64::MAX,
        "other",
    ));
}

fn detail(plan_id: &str, task: &Task) -> serde_json::Value {
    let attempt = task.execution_attempts[0].clone();
    let command = task_report_command(plan_id, &task.id);
    let prompt = build_prompt(
        plan_id,
        task,
        &attempt,
        "test-provider",
        None,
        &command,
        &attempt.plan_skill_manifest,
        attempt.authorized_gates.as_deref().unwrap_or(&[]),
    );
    serde_json::to_value(attempt_view(
        plan_id,
        &task.id,
        &task.milestones,
        attempt,
        Some(prompt),
    ))
    .unwrap()
}

fn detail_read(
    plan_id: &str,
    task: &crate::service_plan::task_attempts::read::TaskAttemptRead,
) -> serde_json::Value {
    let attempt = task.execution_attempts[0].clone();
    let command = task_report_command(plan_id, &task.id);
    let prompt = crate::service_plan::task_attempts::build_prompt_read(
        plan_id,
        (task.prompt.as_ref().unwrap(), task.milestones.as_slice()),
        &attempt,
        "test-provider",
        None,
        &command,
        &attempt.plan_skill_manifest,
        attempt.authorized_gates.as_deref().unwrap_or(&[]),
    );
    serde_json::to_value(attempt_view(
        plan_id,
        &task.id,
        &task.milestones,
        attempt,
        Some(prompt),
    ))
    .unwrap()
}

#[test]
fn attempt_projection_keeps_exact_alias_resolution_latest_report_ties_and_worker_context() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Attempts".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.tasks = (0..3)
        .map(|i| {
            crate::service_plan::load_audit::task(&format!("task_{i}"), TaskStatus::Pending, None)
        })
        .collect();
    decorate(&mut plan);
    plan.tasks[1].handle = Some("collision".into());
    plan.tasks[2].key = Some("COLLISION".into());
    let mut old = crate::service_plan::load_audit::attempt("historic", "other");
    old.closed_at = Some(2);
    plan.tasks[0].execution_attempts.push(old);
    let database = fixtures::database(directory.path(), &plan);
    for reference in ["task_0", "target", "key-1"] {
        let task = database
            .task_attempt_projection(&plan.id, reference, Some("current"))
            .unwrap();
        assert_eq!(task.execution_attempts.len(), 1);
        assert_eq!(task.milestones.len(), 7);
        assert_eq!(
            detail_read(&plan.id, &task),
            detail(&plan.id, &plan.tasks[0])
        );
    }
    let listed = database
        .task_attempt_projection(&plan.id, "target", None)
        .unwrap();
    assert_eq!(listed.milestones.len(), 2);
    let actual = listed
        .execution_attempts
        .iter()
        .cloned()
        .map(|attempt| {
            serde_json::to_value(attempt_view(
                &plan.id,
                &listed.id,
                &listed.milestones,
                attempt,
                None,
            ))
            .unwrap()
        })
        .collect::<Vec<_>>();
    let expected = plan.tasks[0]
        .execution_attempts
        .iter()
        .cloned()
        .map(|attempt| {
            serde_json::to_value(attempt_view(
                &plan.id,
                &plan.tasks[0].id,
                &plan.tasks[0].milestones,
                attempt,
                None,
            ))
            .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    assert_eq!(actual[0]["last_report"], "STATUS: in_review\nlatest report");
    assert!(matches!(
        database.task_attempt_projection(&plan.id, "collision", None),
        Err(PlanError::TaskNotFound)
    ));
    assert!(matches!(
        database.task_attempt_projection("absent", "task_0", None),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.task_attempt_projection(&plan.id, "target", Some("absent")),
        Err(PlanError::TaskNotFound)
    ));
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.session_model',42) WHERE plan_id=?1 AND attempt_id='current'",[&plan.id]).unwrap();
    assert!(matches!(
        database.task_attempt_projection(&plan.id, "target", Some("current")),
        Err(PlanError::Quarantined { .. })
    ));
}

#[test]
fn worker_title_and_planner_queries_preserve_unsigned_sort_duplicates_filters_and_archived_replay()
{
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Readers".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pl_source".into();
    plan.updated_at = 1;
    plan.tasks = vec![crate::service_plan::load_audit::task(
        "task",
        TaskStatus::Done,
        None,
    )];
    decorate(&mut plan);
    plan.tasks[0].execution_attempts.clear();
    for (index, title, assigned, reported, session) in [
        (0, " Z ", Some(u64::MAX), None, "session"),
        (1, "\u{2003}Alpha\u{2003}", Some(u64::MAX), None, "session"),
        (2, "Alpha", Some(u64::MAX), None, "session"),
        (3, "Before", None, Some(u64::MAX - 1), "session"),
        (4, "Ω", Some(0), Some(u64::MAX), "other"),
        (5, "\u{2003}", Some(u64::MAX), None, "session"),
    ] {
        let mut attempt =
            crate::service_plan::load_audit::attempt(&format!("title-{index}"), "worker");
        attempt.role = Some("worker".into());
        attempt.executor_title = Some(title.into());
        attempt.assigned_at = assigned;
        attempt.reported_at = reported;
        attempt.session_id = Some(session.into());
        attempt.closed_at = Some(2);
        attempt.retired_at = Some(1);
        plan.tasks[0].execution_attempts.push(attempt);
    }
    let database = fixtures::database(directory.path(), &plan);
    let mut archived = plan.clone();
    archived.id = "pl_archived".into();
    archived.status = PlanStatus::Archived;
    archived.updated_at = 2;
    database.persist_plan_rows(&archived, None).unwrap();
    assert_eq!(
        database
            .recent_worker_titles(" workspace ", Some(" session "), 2)
            .unwrap(),
        vec!["Alpha", "Alpha"]
    );
    assert_eq!(
        database
            .recent_worker_titles("workspace", None, 10)
            .unwrap(),
        vec!["Alpha", "Alpha", "Z", "Before", "Ω"]
    );
    assert!(database
        .recent_worker_titles("elsewhere", None, 10)
        .unwrap()
        .is_empty());
    let expected = [&archived, &plan]
        .into_iter()
        .filter_map(|plan| planner_replay_candidate(plan, "planner"))
        .collect::<Vec<_>>();
    assert_eq!(database.planner_replay_rows("planner").unwrap(), expected);
    assert!(database.planner_replay_rows("nobody").unwrap().is_empty());
    database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.planner_requests[0].state','invalid') WHERE id=?1",[&plan.id]).unwrap();
    assert!(database.planner_replay_rows("planner").is_err());
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.executor_title',42) WHERE plan_id=?1",[&plan.id]).unwrap();
    assert!(database
        .recent_worker_titles("workspace", None, 10)
        .is_err());
}

#[test]
fn worker_readers_bound_selected_attempt_history_titles_and_replay_materialization() {
    let mut prior = None;
    for (strides, attempts, extra, history) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        decorate(&mut plan);
        plan.tasks[1].body.push_str(&"x".repeat(extra));
        plan.planner_requests[0]
            .payload
            .push_str(&"x".repeat(extra));
        plan.tasks[0].milestones.push(milestone(
            "ignored-note",
            "note",
            &"x".repeat(extra),
            1,
            "other",
        ));
        for i in 0..history {
            plan.tasks[0].milestones.push(milestone(
                &format!("old-{i}"),
                "note",
                "Old irrelevant history",
                1,
                "other",
            ));
            let mut old =
                crate::service_plan::load_audit::attempt(&format!("old-{i}"), "old-worker");
            old.closed_at = Some(1);
            old.role = None;
            plan.tasks[0].execution_attempts.push(old);
        }
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        super::super::super::super::store_sync::take_decoded_row_bytes();
        let full = database.load_plan(&plan.id).unwrap().unwrap();
        let baseline = super::super::super::super::store_sync::take_decoded_row_bytes();
        take_metrics();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let task = database
            .task_attempt_projection(&plan.id, &plan.tasks[0].id, Some("current"))
            .unwrap();
        let selected = detail_read(&plan.id, &task);
        assert_eq!(selected, detail(&plan.id, &full.tasks[0]));
        let titles = database.recent_worker_titles("workspace", None, 2).unwrap();
        assert_eq!(titles, vec!["Current"]);
        let replay = database.planner_replay_rows("planner").unwrap();
        assert_eq!(
            replay,
            vec![planner_replay_candidate(&full, "planner").unwrap()]
        );
        let response_bytes = serde_json::to_vec(&json!([selected, titles, replay]))
            .unwrap()
            .len();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(metrics.queries, 7);
        assert!(metrics.materialized_bytes < 131072);
        assert!(decoded < 131072);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=worker_reads tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra} unselected_target_history={history} baseline_decoded_json_bytes={baseline} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior)
        }
        prior = Some(measured);
    }
}

#[test]
fn attempt_projection_keeps_task_prompt_fields_and_attempt_identity_in_one_commit() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Race".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.tasks = vec![crate::service_plan::load_audit::task(
        "task",
        TaskStatus::Pending,
        None,
    )];
    decorate(&mut plan);
    plan.tasks[0].name = "A".into();
    plan.tasks[0].execution_attempts[0].executor_title = Some("A".into());
    let database = fixtures::database(directory.path(), &plan);
    let path = database.path();
    let id = plan.id.clone();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        for index in 0..16 {
            let value = if index % 2 == 0 { "B" } else { "A" };
            let tx = connection.transaction().unwrap();
            tx.execute(
                "UPDATE plan_tasks SET row_json=json_set(row_json,'$.name',?2) WHERE plan_id=?1",
                params![id, value],
            )
            .unwrap();
            tx.execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.executor_title',?2) WHERE plan_id=?1",params![id,value]).unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let task = database
            .task_attempt_projection(&plan.id, "task", Some("current"))
            .unwrap();
        assert_eq!(
            Some(task.prompt.as_ref().unwrap().name.as_str()),
            task.execution_attempts[0].executor_title.as_deref()
        );
        barrier.wait();
    }
    writer.join().unwrap();
}
