use super::*;
use crate::plan::{PlanStatus, TaskExecutorTermination, TaskStatus};

fn authority_plan(root: &std::path::Path) -> Plan {
    let mut plan = Plan::new(
        "Authority".into(),
        root.display().to_string(),
        OrchestratorRef::default(),
    );
    let mut task =
        crate::service_plan::load_audit::task("task_exact", TaskStatus::Paused, Some("worker"));
    task.handle = Some("named".into());
    task.key = Some("COD-1".into());
    task.interruption.owner_epoch = 7;
    task.interruption.owner_tab_id = Some("worker".into());
    task.interruption.owner_session_id = Some("session".into());
    let mut attempt = crate::service_plan::load_audit::attempt("current", "worker");
    attempt.session_id = Some("session".into());
    attempt.session_binding_nonce = Some("nonce".into());
    attempt.executor_termination = Some(TaskExecutorTermination {
        attempt_id: attempt.id.clone(),
        executor_tab_id: attempt.executor_tab_id.clone(),
        session_id: attempt.session_id.clone(),
        session_binding_nonce: attempt.session_binding_nonce.clone(),
        confirmed_at: 9,
    });
    task.execution_attempts = vec![attempt];
    plan.tasks = vec![task];
    plan
}

#[test]
fn unplanned_work_facet_read_selects_only_exact_declared_key() {
    let directory = tempfile::tempdir().unwrap();
    let plan = authority_plan(directory.path());
    let database = fixtures::database(directory.path(), &plan);
    for (facets, expected) in [
        (r#"[]"#, false),
        (r#"[{"key":"work"}]"#, true),
        (r#"[{"key":"Work"},{"key":"other","label":"work"}]"#, false),
    ] {
        database.connection.lock().execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.tag_facets',json(?1)) WHERE id=?2",
            rusqlite::params![facets,plan.id],
        ).unwrap();
        assert_eq!(
            database.plan_work_facet_projection(&plan.id).unwrap(),
            expected
        );
    }
    assert!(matches!(
        database.plan_work_facet_projection("missing"),
        Err(PlanError::NotFound)
    ));
    database
        .connection
        .lock()
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.tag_facets',json('[\"broken\"]')) WHERE id=?1",
            [&plan.id],
        )
        .unwrap();
    assert!(database.plan_work_facet_projection(&plan.id).is_err());
}

#[test]
fn owner_epoch_selects_exact_aliases_and_preserves_partial_interruption_fields() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    let database = fixtures::database(directory.path(), &plan);
    let expected = database.task_owner_epoch_projection("task_exact").unwrap();
    assert_eq!(expected.owner_epoch, 7);
    assert_eq!(expected.owner_session_id.as_deref(), Some("session"));
    for reference in ["NAMED", "cod-1"] {
        assert_eq!(
            database.task_owner_epoch_projection(reference).unwrap(),
            expected
        );
    }
    let mut collision = plan.tasks[0].clone();
    collision.id = "collision".into();
    collision.handle = Some("task_exact".into());
    plan.tasks.push(collision);
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database.task_owner_epoch_projection("task_exact").unwrap(),
        expected
    );
    assert!(matches!(
        database.task_owner_epoch_projection("COD-1"),
        Err(PlanError::TaskNotFound)
    ));
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.interruption',json(?1)) WHERE task_id='task_exact'", [r#"{"owner_epoch":8,"owner_tab_id":"worker","handoff_required":true}"#]).unwrap();
    let partial = database.task_owner_epoch_projection("task_exact").unwrap();
    assert_eq!(partial.owner_epoch, 8);
    assert!(partial.handoff_required);
    assert_eq!(partial.owner_session_id, None);
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.interruption.owner_epoch','invalid') WHERE task_id='task_exact'", []).unwrap();
    assert!(database.task_owner_epoch_projection("task_exact").is_err());
    plan.status = PlanStatus::Archived;
    database.persist_plan_rows(&plan, None).unwrap();
    assert!(matches!(
        database.task_owner_epoch_projection("task_exact"),
        Err(PlanError::TaskNotFound)
    ));
}

#[test]
fn skill_admission_rejects_stale_or_orphaned_and_duplicate_attempts() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    let database = fixtures::database(directory.path(), &plan);
    let expected = Some((plan.id.clone(), "task_exact".into(), "current".into()));
    assert_eq!(
        database
            .current_skill_attempt("worker", "session", "nonce")
            .unwrap(),
        expected
    );
    for (session, nonce) in [("stale", "nonce"), ("session", "stale")] {
        assert!(database
            .current_skill_attempt("worker", session, nonce)
            .is_err());
    }
    for (closed, retired) in [(Some(3), None), (None, Some(3))] {
        plan.tasks[0].execution_attempts[0].closed_at = closed;
        plan.tasks[0].execution_attempts[0].retired_at = retired;
        database.persist_plan_rows(&plan, None).unwrap();
        assert!(database
            .current_skill_attempt("worker", "session", "nonce")
            .is_err());
    }
    plan.tasks[0].execution_attempts[0].closed_at = None;
    plan.tasks[0].execution_attempts[0].retired_at = None;
    plan.tasks[0].assigned_tab_id = None;
    database.persist_plan_rows(&plan, None).unwrap();
    assert!(database
        .current_skill_attempt("worker", "session", "nonce")
        .is_err());
    plan.tasks[0].execution_attempts[0].competition_group_id = Some("competition".into());
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database
            .current_skill_attempt("worker", "session", "nonce")
            .unwrap(),
        expected
    );
    let mut duplicate = plan.tasks[0].execution_attempts[0].clone();
    duplicate.id = "duplicate".into();
    plan.tasks[0].execution_attempts.push(duplicate);
    database.persist_plan_rows(&plan, None).unwrap();
    assert!(database
        .current_skill_attempt("worker", "session", "nonce")
        .is_err());
    plan.tasks[0].status = TaskStatus::Done;
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database
            .current_skill_attempt("worker", "session", "nonce")
            .unwrap(),
        None
    );
}

#[test]
fn executor_termination_requires_exact_attempt_and_receipt_identity() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    let source = plan.tasks[0].execution_attempts[0].clone();
    let database = fixtures::database(directory.path(), &plan);
    assert!(database
        .executor_termination_confirmed(&plan.id, "named", (&source).into())
        .unwrap());
    for field in ["id", "tab", "session", "nonce", "peer"] {
        let mut changed = source.clone();
        match field {
            "id" => changed.id = "other".into(),
            "tab" => changed.executor_tab_id = "other".into(),
            "session" => changed.session_id = Some("other".into()),
            "nonce" => changed.session_binding_nonce = Some("other".into()),
            _ => changed.mesh_peer = Some("remote".into()),
        }
        assert!(!database
            .executor_termination_confirmed(&plan.id, "task_exact", (&changed).into())
            .unwrap());
    }
    plan.tasks[0].execution_attempts[0]
        .executor_termination
        .as_mut()
        .unwrap()
        .session_binding_nonce = Some("stale".into());
    database.persist_plan_rows(&plan, None).unwrap();
    assert!(!database
        .executor_termination_confirmed(&plan.id, "task_exact", (&source).into())
        .unwrap());
    assert!(matches!(
        database.executor_termination_confirmed("missing", "task_exact", (&source).into()),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.executor_termination_confirmed(&plan.id, "missing", (&source).into()),
        Err(PlanError::TaskNotFound)
    ));
}

#[test]
fn authority_reads_ignore_unrequested_prose_and_selected_and_unrelated_history_growth() {
    let mut prior = None;
    for (strides, attempts, body, history) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].assigned_tab_id = Some("worker".into());
        plan.tasks[0].interruption.owner_epoch = 7;
        plan.tasks[0].interruption.owner_tab_id = Some("worker".into());
        plan.tasks[0].interruption.owner_session_id = Some("session".into());
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let source = plan.tasks[0].execution_attempts[0].clone();
        for index in 0..history {
            let mut old = source.clone();
            old.id = format!("unselected-{index}");
            old.closed_at = Some(1);
            plan.tasks[0].execution_attempts.push(old);
        }
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        super::super::super::super::store_sync::take_decoded_row_bytes();
        database.load_plan(&plan.id).unwrap().unwrap();
        let baseline = super::super::super::super::store_sync::take_decoded_row_bytes();
        take_metrics();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let owner = database.task_owner_epoch_projection("task_0000").unwrap();
        let current = database
            .current_skill_attempt("worker", "session", "nonce")
            .unwrap();
        let terminated = database
            .executor_termination_confirmed(&plan.id, "task_0000", (&source).into())
            .unwrap();
        assert_eq!(owner.owner_epoch, 7);
        assert_eq!(
            current,
            Some((plan.id.clone(), "task_0000".into(), "current".into()))
        );
        assert!(!terminated);
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(owner, current, terminated))
            .unwrap()
            .len();
        assert_eq!(metrics.queries, 5);
        assert_eq!(decoded, 0);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=task_authority tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} baseline_decoded_json_bytes={baseline} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn authority_identity_and_termination_reads_observe_committed_snapshots() {
    let directory = tempfile::tempdir().unwrap();
    let plan = authority_plan(directory.path());
    let source = plan.tasks[0].execution_attempts[0].clone();
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
        for index in 0..32 {
            let (epoch, owner, nonce) = if index % 2 == 0 {
                (8, "other", "other")
            } else {
                (7, "worker", "nonce")
            };
            let tx = connection.transaction().unwrap();
            tx.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.interruption.owner_epoch',?2,'$.interruption.owner_tab_id',?3) WHERE plan_id=?1",params![id,epoch,owner]).unwrap();
            tx.execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.session_binding_nonce',?2,'$.executor_termination.session_binding_nonce',?2) WHERE plan_id=?1",params![id,nonce]).unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..32 {
        barrier.wait();
        let owner = database.task_owner_epoch_projection("task_exact").unwrap();
        assert!(matches!(
            (owner.owner_epoch, owner.owner_tab_id.as_deref()),
            (7, Some("worker")) | (8, Some("other"))
        ));
        if let Ok(Some(current)) = database.current_skill_attempt("worker", "session", "nonce") {
            assert_eq!(
                current,
                (plan.id.clone(), "task_exact".into(), "current".into())
            );
        }
        database
            .executor_termination_confirmed(&plan.id, "task_exact", (&source).into())
            .unwrap();
        barrier.wait();
    }
    writer.join().unwrap();
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plan_task_attempts")
        .unwrap();
    assert!(database
        .current_skill_attempt("worker", "session", "nonce")
        .is_err());
    assert!(database
        .executor_termination_confirmed(&plan.id, "task_exact", (&source).into())
        .is_err());
}

#[test]
fn context_files_select_only_requested_task_metadata_across_growth_variants() {
    let mut prior = None;
    for (strides, attempts, body, history) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].files.edit = vec!["src/owned.rs".into()];
        plan.tasks[0].files.read = vec!["docs/contract.md".into()];
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let source = plan.tasks[0].execution_attempts[0].clone();
        for index in 0..history {
            let mut old = source.clone();
            old.id = format!("unselected-{index}");
            old.closed_at = Some(1);
            plan.tasks[0].execution_attempts.push(old);
        }
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let files = database
            .task_context_files_projection("fixture-0000")
            .unwrap();
        assert_eq!(files.plan_id, plan.id);
        assert_eq!(files.task_id, "task_0000");
        assert_eq!(files.files, plan.tasks[0].files);
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&files).unwrap().len();
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 256);
        assert!(decoded < 256);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=task_context_files tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
        assert!(matches!(
            database.task_context_files_projection("missing"),
            Err(PlanError::TaskNotFound)
        ));
        database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.files.edit',42) WHERE task_id='task_0000'",[]).unwrap();
        assert!(database.task_context_files_projection("task_0000").is_err());
    }
}

#[test]
fn plan_admission_preserves_mode_and_approval_decisions_and_missing_errors() {
    use crate::plan::{PlanMode, PlanningState};
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    let database = fixtures::database(directory.path(), &plan);
    for mode in [
        PlanMode::Planning,
        PlanMode::Approved,
        PlanMode::Execution,
        PlanMode::Paused,
        PlanMode::Complete,
    ] {
        for state in [
            PlanningState::Drafting,
            PlanningState::StructureReady,
            PlanningState::Approved,
        ] {
            plan.mode = mode.clone();
            plan.planning_state = state;
            database.persist_plan_rows(&plan, None).unwrap();
            let selected = database.plan_task_admission_projection(&plan.id).unwrap();
            assert_eq!(
                selected.execution_tasks_unlocked(),
                plan.execution_tasks_unlocked()
            );
            assert_eq!(selected.planning_state, plan.planning_state);
        }
    }
    assert!(matches!(
        database.plan_task_admission_projection("missing"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.ensure_plan_exists("missing"),
        Err(PlanError::NotFound)
    ));
    database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.planning_state','invalid') WHERE id=?1",[&plan.id]).unwrap();
    assert!(database.plan_task_admission_projection(&plan.id).is_err());
}

#[test]
fn unplanned_section_prefers_parent_then_first_root_and_distinguishes_absent_plan() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    plan.sections = serde_json::from_value(json!([
        {"id":"first","title":"First","subsections":[{"id":"nested","title":"Nested"}]},
        {"id":"second","title":"Second"}
    ]))
    .unwrap();
    plan.tasks[0].section_ids = vec!["second".into(), "nested".into()];
    let database = fixtures::database(directory.path(), &plan);
    assert_eq!(
        database
            .unplanned_task_section_projection(&plan.id, Some("task_exact"))
            .unwrap()
            .as_deref(),
        Some("second")
    );
    for parent in [None, Some("missing")] {
        assert_eq!(
            database
                .unplanned_task_section_projection(&plan.id, parent)
                .unwrap()
                .as_deref(),
            Some("first")
        );
    }
    plan.tasks[0].section_ids.clear();
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database
            .unplanned_task_section_projection(&plan.id, Some("task_exact"))
            .unwrap()
            .as_deref(),
        Some("first")
    );
    plan.sections.clear();
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database
            .unplanned_task_section_projection(&plan.id, None)
            .unwrap(),
        None
    );
    assert!(matches!(
        database.unplanned_task_section_projection("missing", None),
        Err(PlanError::NotFound)
    ));
}

#[test]
fn admission_target_and_unplanned_section_reads_have_fixed_materialization() {
    let mut prior = None;
    for (strides, attempts, body, history) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].section_ids = vec!["selected".into()];
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let source = plan.tasks[0].execution_attempts[0].clone();
        for index in 0..history {
            let mut old = source.clone();
            old.id = format!("unselected-{index}");
            old.closed_at = Some(1);
            plan.tasks[0].execution_attempts.push(old);
        }
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        database.ensure_plan_exists(&plan.id).unwrap();
        let admission = database.plan_task_admission_projection(&plan.id).unwrap();
        let target = database
            .task_authority_target_projection("fixture-0000")
            .unwrap();
        let section = database
            .unplanned_task_section_projection(&plan.id, Some("task_0000"))
            .unwrap();
        assert_eq!(target.task_id, "task_0000");
        assert_eq!(section.as_deref(), Some("selected"));
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(admission, target, section))
            .unwrap()
            .len();
        assert_eq!(metrics.queries, 4);
        assert!(metrics.materialized_bytes < 256);
        assert!(decoded < 256);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=task_admission tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn worker_transfer_projection_keeps_unique_counts_and_fixed_selected_metadata() {
    use crate::plan::{PlanMode, TaskLaneMetadata};
    use crate::service_plan::mutations::agent_lifecycle::VerifiedWorkerTransferBinding;
    let mut prior = None;
    for (strides, attempts, body, history) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.mode = PlanMode::Execution;
        plan.team_id = Some("team".into());
        let task = &mut plan.tasks[0];
        task.status = TaskStatus::InProgress;
        task.assigned_tab_id = Some("worker".into());
        task.lane = TaskLaneMetadata {
            tab_id: Some("worker".into()),
            workspace_id: Some("workspace".into()),
            worktree_path: Some("/fixture/lane".into()),
            branch: Some("lane/worker".into()),
            base: Some("base".into()),
            ..Default::default()
        };
        task.interruption.owner_epoch = 7;
        task.interruption.owner_tab_id = Some("worker".into());
        task.interruption.owner_session_id = Some("session".into());
        let attempt = &mut task.execution_attempts[0];
        attempt.provider = Some("provider".into());
        attempt.agent_type = Some("provider".into());
        attempt.worktree_path = task.lane.worktree_path.clone();
        attempt.branch = task.lane.branch.clone();
        attempt.base = task.lane.base.clone();
        attempt.acknowledged_at = Some(4);
        attempt.interruption.owner_epoch = 7;
        let source = attempt.clone();
        for index in 0..history {
            let mut old = source.clone();
            old.id = format!("unselected-{index}");
            old.closed_at = Some(1);
            task.execution_attempts.push(old);
        }
        task.body.push_str(&"x".repeat(body));
        task.lane.cleanup_hints.push("x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let expected = VerifiedWorkerTransferBinding {
            plan_id: plan.id.clone(),
            team_id: "team".into(),
            owner_tab_id: "owner".into(),
            owner_session_id: "owner-session".into(),
            task_id: "task_0000".into(),
            attempt_id: "current".into(),
            worker_tab_id: "worker".into(),
            workspace_id: "workspace".into(),
            worktree_path: "/fixture/lane".into(),
            session_id: "session".into(),
            binding_nonce: "nonce".into(),
            provider: "provider".into(),
            owner_epoch: 7,
        };
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let read = database.worker_transfer_projection(&expected).unwrap();
        assert_eq!(
            (
                read.assignments,
                read.lanes,
                read.worktrees,
                read.open_attempts
            ),
            (1, 1, 1, 1)
        );
        assert!(read.subject.as_ref().unwrap().matches(&expected));
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&read).unwrap().len();
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 2048);
        assert!(decoded < 2048);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=worker_transfer tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
        database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.acknowledged_at','invalid') WHERE attempt_id='current'",[]).unwrap();
        assert!(database.worker_transfer_projection(&expected).is_err());
    }
}

#[test]
fn task_name_projection_does_not_materialize_body_or_history() {
    let mut prior = None;
    for (strides, attempts, body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let name = database.task_name_projection("fixture-0000").unwrap();
        assert_eq!(name, plan.tasks[0].name);
        let metrics = take_metrics();
        assert_eq!(metrics.queries, 1);
        assert_eq!(metrics.materialized_bytes, name.len());
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(decoded, 0);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        let response = serde_json::to_vec(&name).unwrap().len();
        println!("fixture={} consumer=task_name tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
        assert!(matches!(
            database.task_name_projection("missing"),
            Err(PlanError::TaskNotFound)
        ));
    }
}

#[test]
fn startup_projection_preserves_status_and_exact_historical_owner_decisions() {
    let directory = tempfile::tempdir().unwrap();
    let mut base = authority_plan(directory.path());
    base.tasks[0].status = TaskStatus::Pending;
    base.tasks[0].execution_attempts[0].executor_title = Some("Worker".into());
    let mut variants = vec![base.clone()];
    for status in [
        TaskStatus::InProgress,
        TaskStatus::Blocked,
        TaskStatus::InReview,
        TaskStatus::Paused,
        TaskStatus::Done,
        TaskStatus::Cancelled,
    ] {
        let mut plan = base.clone();
        plan.tasks[0].status = status;
        variants.push(plan);
    }
    type Change = fn(&mut crate::plan::Task);
    let changes: &[Change] = &[
        |task| task.assigned_tab_id = None,
        |task| task.assigned_tab_id = Some("other".into()),
        |task| task.execution_attempts.clear(),
        |task| task.execution_attempts[0].executor_title = None,
        |task| task.execution_attempts[0].acknowledged_at = Some(4),
        |task| task.execution_attempts[0].reported_at = Some(4),
        |task| task.execution_attempts[0].report = Some(Default::default()),
        |task| task.execution_attempts[0].competition_group_id = Some("group".into()),
        |task| task.execution_attempts[0].closed_at = Some(4),
        |task| task.execution_attempts[0].retired_at = Some(4),
    ];
    for change in changes {
        let mut plan = base.clone();
        change(&mut plan.tasks[0]);
        variants.push(plan);
    }
    for (group, session, retired, closed, executor) in [
        (None, Some("session"), None, None, "worker"),
        (Some("group"), Some("session"), None, None, "worker"),
        (Some("group"), None, None, None, "worker"),
        (Some("group"), Some("\u{2003}"), None, None, "worker"),
        (Some("group"), Some("session"), Some(2), None, "worker"),
        (Some("group"), Some("session"), None, Some(2), "worker"),
        (Some("group"), Some("session"), None, None, "other"),
    ] {
        let mut plan = base.clone();
        let mut prior = plan.tasks[0].execution_attempts[0].clone();
        prior.competition_group_id = group.map(str::to_string);
        prior.session_id = session.map(str::to_string);
        prior.retired_at = retired;
        prior.closed_at = closed;
        prior.executor_tab_id = executor.into();
        plan.tasks[0].execution_attempts.insert(0, prior);
        variants.push(plan);
    }
    for group in [None, Some("newer-group")] {
        let mut plan = base.clone();
        let mut retired = plan.tasks[0].execution_attempts[0].clone();
        retired.id = "newer-retired".into();
        retired.retired_at = Some(2);
        retired.competition_group_id = group.map(str::to_string);
        plan.tasks[0].execution_attempts.push(retired);
        variants.push(plan);
    }
    let database = fixtures::database(directory.path(), &base);
    for (index, plan) in variants.iter().enumerate() {
        database.persist_plan_rows(plan, None).unwrap();
        let expected =
            crate::service_plan::task_dispatch::current_dispatch_startup_owner(&plan.tasks[0]);
        assert_eq!(
            database
                .dispatch_startup_owner_projection(&plan.id, "named")
                .unwrap(),
            expected,
            "variant {index}"
        );
        assert_eq!(
            database.task_status_projection(&plan.id, "named").unwrap(),
            plan.tasks[0].status,
            "variant {index}"
        );
    }
    database.persist_plan_rows(&base, None).unwrap();
    assert_eq!(
        database
            .dispatch_startup_owner_projection("missing", "named")
            .unwrap(),
        None
    );
    assert!(matches!(
        database.task_status_projection("missing", "named"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.task_status_projection(&base.id, "missing"),
        Err(PlanError::TaskNotFound)
    ));
    let mut collision = base.tasks[0].clone();
    collision.id = "second".into();
    base.tasks.push(collision);
    database.persist_plan_rows(&base, None).unwrap();
    assert_eq!(
        database
            .dispatch_startup_owner_projection(&base.id, "named")
            .unwrap(),
        None
    );
    assert!(matches!(
        database.task_status_projection(&base.id, "named"),
        Err(PlanError::TaskNotFound)
    ));
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.acknowledged_at','invalid') WHERE task_id='task_exact'",[]).unwrap();
    assert!(database
        .dispatch_startup_owner_projection(&base.id, "task_exact")
        .is_err());
}

#[test]
fn startup_owner_and_status_queries_ignore_unselected_history_and_bodies() {
    let mut prior = None;
    for (strides, attempts, body, history) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        let task = &mut plan.tasks[0];
        task.assigned_tab_id = Some("worker".into());
        task.execution_attempts[0].executor_title = Some("Worker".into());
        let source = task.execution_attempts[0].clone();
        for index in 0..history {
            let mut old = source.clone();
            old.id = format!("closed-{index}");
            old.closed_at = Some(2);
            task.execution_attempts.push(old);
        }
        task.body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let status = database
            .task_status_projection(&plan.id, "task_0000")
            .unwrap();
        let owner = database
            .dispatch_startup_owner_projection(&plan.id, "task_0000")
            .unwrap()
            .unwrap();
        assert_eq!(status, TaskStatus::Pending);
        assert_eq!(owner.attempt_id, "current");
        assert_eq!(owner.tab_id, "worker");
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(status, owner.attempt_id, owner.tab_id))
            .unwrap()
            .len();
        assert_eq!(metrics.queries, 2);
        assert!(metrics.materialized_bytes < 2048);
        assert!(decoded < 2048);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=startup_owner tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}
mod checkpoint;
mod continuity;
mod creation;
mod gates;
mod restart;
mod restoration;
mod review;
mod runtime_restore;
mod runtime_session;
mod split;
mod startup_ownership;

mod provider_interruption;

mod remote;

mod review_followup;

mod lane_admission;

mod batch;

mod shared_inspection;

mod review_roles;

mod plan_metadata;

mod respawn_source;

mod worker_loss;

mod ack;

mod lifecycle;

mod launch;

mod deadline;

mod live_bindings;

mod worker_preparation;

mod closed_cleanup;

mod dispatch_preflight;
mod task_display;
