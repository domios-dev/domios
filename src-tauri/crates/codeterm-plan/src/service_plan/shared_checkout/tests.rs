use super::*;
use crate::plan::{OrchestratorRef, TaskExecutionAttempt};
use std::process::Command;
mod candidates;
mod history;
mod inspection;

fn task(id: &str, edits: &[&str], live: bool) -> Task {
    let mut task: Task = serde_json::from_value(serde_json::json!({
        "id": id, "name": id, "type": "code", "status": "pending",
        "files": {"edit": edits}
    }))
    .unwrap();
    if live {
        task.execution_attempts.push(TaskExecutionAttempt {
            id: format!("attempt-{id}"),
            placement: WorkerPlacement::Shared,
            ..Default::default()
        });
    }
    task
}

fn plan(path: &str) -> Plan {
    Plan::new(
        "fixture".into(),
        path.into(),
        OrchestratorRef::from_tab("owner"),
    )
}

fn peer(plan: Plan) -> CheckoutPeerRead {
    CheckoutPeerRead {
        checkout: canonical(&plan),
        id: plan.id,
        tasks: plan
            .tasks
            .into_iter()
            .map(|task| {
                let execution_attempts = task
                    .execution_attempts
                    .iter()
                    .filter(|attempt| attempt_reserves_checkout_writer(&task, attempt))
                    .map(|attempt| CheckoutAttemptRead {
                        id: attempt.id.clone(),
                        placement: attempt.placement,
                        checkout_path: attempt
                            .checkout_path
                            .clone()
                            .or_else(|| attempt.worktree_path.clone()),
                    })
                    .collect();
                CheckoutTaskRead {
                    id: task.id,
                    files: task.files,
                    execution_attempts,
                }
            })
            .collect(),
    }
}

#[test]
fn shared_placement_defaults_to_disjoint_files_and_reports_lane_reasons() {
    let mut plan = plan("/checkout");
    let mine = task("mine", &["src/a.rs"], false);
    plan.tasks.push(task("foreign", &["src/b.rs"], true));
    assert_eq!(
        choose_placement(&[peer(plan.clone())], &plan, &mine, None)
            .unwrap()
            .0,
        WorkerPlacement::Shared
    );
    assert_eq!(
        choose_placement(&[peer(plan.clone())], &plan, &mine, Some("lane"))
            .unwrap()
            .0,
        WorkerPlacement::IsolatedLane
    );
    plan.tasks[0].files.edit = vec!["src".into()];
    let decision = choose_placement(&[peer(plan.clone())], &plan, &mine, None).unwrap();
    assert_eq!(decision.0, WorkerPlacement::IsolatedLane);
    assert!(decision.1.contains("foreign"));
    assert!(
        choose_placement(&[peer(plan.clone())], &plan, &mine, Some("shared"))
            .unwrap_err()
            .contains("foreign")
    );
    let mut breaking = mine.clone();
    breaking.tags.push("build-breaking".into());
    assert!(choose_placement(&[], &plan, &breaking, None)
        .unwrap()
        .1
        .contains("build-breaking"));
    assert!(choose_placement(&[], &plan, &breaking, Some("shared")).is_err());
    assert_eq!(
        choose_placement(&[], &plan, &breaking, Some("lane"))
            .unwrap()
            .0,
        WorkerPlacement::IsolatedLane
    );
    assert!(choose_placement(&[], &plan, &task("empty", &[], false), None).is_err());
}

#[test]
fn missing_shared_files_provides_runnable_declaration_and_lane_commands() {
    let plan = plan("/checkout");
    let task = task("opaque-task", &[], false);
    let error = choose_placement(&[], &plan, &task, None).unwrap_err();
    assert!(error.contains(&format!(
        "domios plan task files {} {} --edit-file <path>",
        plan.id, task.id
    )));
    assert!(error.contains(&format!(
        "domios plan task dispatch {} {} --execution-intent mutating --placement lane",
        plan.id, task.id
    )));
}

#[test]
fn shared_ownership_checks_prefix_boundaries_cross_plan_and_closed_attempts() {
    let plan = plan("/checkout");
    let mine = task("mine", &["src"], false);
    let mut foreign = self::plan("/checkout");
    foreign.id = "other-plan".into();
    foreign.tasks.push(task("foreign", &["src/file.rs"], true));
    assert!(
        validate_shared_files(&[peer(foreign.clone())], &plan, &mine, &mine.files)
            .unwrap_err()
            .contains("other-plan")
    );
    foreign.tasks[0].files.edit = vec!["src-other/file.rs".into()];
    assert!(validate_shared_files(&[peer(foreign.clone())], &plan, &mine, &mine.files).is_ok());
    foreign.tasks[0].files.edit = vec!["src/file.rs".into()];
    foreign.tasks[0].execution_attempts[0].closed_at = Some(4);
    assert!(validate_shared_files(&[peer(foreign)], &plan, &mine, &mine.files).is_ok());
    for edit in ["../src", "/src", "C:/src", "src/../foreign", ".git/hooks"] {
        assert!(task("invalid", &[edit], false)
            .files
            .shared_edit_paths()
            .is_err());
    }
}

#[test]
#[cfg(unix)]
fn automatic_placement_refuses_linked_edit_paths_instead_of_isolating_an_external_target() {
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(external.path(), root.path().join("linked")).unwrap();
    let plan = self::plan(root.path().to_str().unwrap());
    let task = task("mine", &["linked/file.rs"], false);
    assert!(choose_placement(&[], &plan, &task, None)
        .unwrap_err()
        .contains("filesystem link"));
}

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

#[test]
fn shared_reservation_serializes_overlap_and_extension_checks() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init"]);
    git(root.path(), &["config", "user.name", "Fixture"]);
    git(
        root.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    std::fs::write(root.path().join("initial"), "initial").unwrap();
    git(root.path(), &["add", "initial"]);
    git(root.path(), &["commit", "-m", "initial"]);
    let store = std::sync::Arc::new(PlanStore::new_for_test(root.path()));
    let mut plan = self::plan(root.path().to_str().unwrap());
    store.save(&mut plan, 1).unwrap();
    let ids: Vec<String> = ["first", "second"]
        .into_iter()
        .map(|name| {
            store
                .add_task(
                    &plan.id,
                    super::super::NewTask {
                        name: name.into(),
                        files: TaskFiles {
                            edit: vec!["shared.rs".into()],
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    2,
                )
                .unwrap()
        })
        .collect();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = ids
        .iter()
        .map(|id| {
            let store = store.clone();
            let barrier = barrier.clone();
            let plan_id = plan.id.clone();
            let id = id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let result = store.begin_task_dispatch_with_reserved_attempt_and_gates(
                    &plan_id,
                    &id,
                    &id,
                    "Fixture",
                    Default::default(),
                    crate::plan::ExecutionIntent::Mutating,
                    None,
                    None,
                    &format!("attempt-{id}"),
                    vec![],
                    WorkerPlacement::Shared,
                    3,
                );
                (id, result)
            })
        })
        .collect();
    let outcomes: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(
        outcomes.iter().filter(|(_, result)| result.is_ok()).count(),
        1
    );
    let winner = &outcomes
        .iter()
        .find(|(_, result)| result.is_ok())
        .unwrap()
        .0;
    let loser = &outcomes
        .iter()
        .find(|(_, result)| result.is_err())
        .unwrap()
        .0;
    store
        .update_task_files(
            &plan.id,
            loser,
            TaskFiles {
                edit: vec!["foreign.rs".into()],
                ..Default::default()
            },
            4,
        )
        .unwrap();
    store
        .begin_task_dispatch_with_reserved_attempt_and_gates(
            &plan.id,
            loser,
            loser,
            "Fixture",
            Default::default(),
            crate::plan::ExecutionIntent::Mutating,
            None,
            None,
            "second-attempt",
            vec![],
            WorkerPlacement::Shared,
            5,
        )
        .unwrap();
    assert!(store
        .extend_task_edit_files(&plan.id, winner, vec!["foreign.rs".into()], 6)
        .is_err());
    store
        .extend_task_edit_files(&plan.id, winner, vec!["new.rs".into()], 7)
        .unwrap();
    let task = store.inspect_task_for_test(winner).unwrap().task;
    assert_eq!(task.files.edit, ["shared.rs", "new.rs"]);
    assert!(store
        .update_task_files(
            &plan.id,
            winner,
            TaskFiles {
                edit: vec!["foreign.rs".into()],
                ..Default::default()
            },
            8
        )
        .is_err());
    assert_eq!(
        store.inspect_task_for_test(winner).unwrap().task.files,
        task.files
    );
    let error = store
        .update_task_files(
            &plan.id,
            winner,
            TaskFiles {
                edit: vec!["new.rs".into()],
                ..Default::default()
            },
            9,
        )
        .unwrap_err();
    assert!(error.to_string().contains("cannot release edit paths"));
    assert_eq!(
        store.inspect_task_for_test(winner).unwrap().task.files,
        task.files
    );
}

#[test]
fn shared_commit_guard_accepts_own_commit_with_foreign_dirt_and_refuses_foreign_or_undeclared_paths(
) {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init"]);
    git(root.path(), &["config", "user.name", "Fixture"]);
    git(
        root.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    std::fs::write(root.path().join("initial"), "initial").unwrap();
    git(root.path(), &["add", "initial"]);
    git(root.path(), &["commit", "-m", "initial"]);
    let mut attempt = TaskExecutionAttempt {
        checkout_path: Some(root.path().display().to_string()),
        shared_start_sha: Some(git(root.path(), &["rev-parse", "HEAD"])),
        ..Default::default()
    };
    let mine = task("mine", &["owned"], false);
    std::fs::write(root.path().join("owned"), "own change").unwrap();
    git(root.path(), &["add", "owned"]);
    git(root.path(), &["commit", "-m", "own"]);
    let own = git(root.path(), &["rev-parse", "HEAD"]);
    std::fs::write(root.path().join("foreign"), "foreign uncommitted").unwrap();
    assert_eq!(
        validate_shared_report(&mine, &attempt, Some(&own)).unwrap(),
        ["owned"]
    );
    git(root.path(), &["add", "foreign"]);
    git(root.path(), &["commit", "-m", "foreign"]);
    let foreign = git(root.path(), &["rev-parse", "HEAD"]);
    assert!(validate_shared_report(&mine, &attempt, Some(&foreign))
        .unwrap_err()
        .contains("foreign"));
    assert!(validate_shared_report(&mine, &attempt, Some(&own)).is_ok());
    std::fs::write(root.path().join("undeclared"), "undeclared").unwrap();
    git(root.path(), &["add", "undeclared"]);
    git(root.path(), &["commit", "-m", "undeclared"]);
    assert!(validate_shared_report(
        &mine,
        &attempt,
        Some(&git(root.path(), &["rev-parse", "HEAD"]))
    )
    .unwrap_err()
    .contains("undeclared"));
    std::fs::write(root.path().join("owned"), "unfinished").unwrap();
    assert!(validate_shared_report(&mine, &attempt, Some(&own))
        .unwrap_err()
        .contains("uncommitted"));
    attempt.shared_start_sha = Some(own.clone());
    assert!(validate_shared_report(&mine, &attempt, Some(&own)).is_err());
}

#[test]
fn shared_report_names_a_commit_reset_away_from_head() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init"]);
    git(root.path(), &["config", "user.name", "Fixture"]);
    git(
        root.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    std::fs::write(root.path().join("owned"), "initial").unwrap();
    git(root.path(), &["add", "owned"]);
    git(root.path(), &["commit", "-m", "initial"]);
    let start = git(root.path(), &["rev-parse", "HEAD"]);
    std::fs::write(root.path().join("owned"), "changed").unwrap();
    git(root.path(), &["commit", "-am", "task"]);
    let reported = git(root.path(), &["rev-parse", "HEAD"]);
    git(root.path(), &["reset", "--hard", &start]);
    let attempt = TaskExecutionAttempt {
        checkout_path: Some(root.path().display().to_string()),
        shared_start_sha: Some(start.clone()),
        ..Default::default()
    };
    let error = validate_shared_report(&task("mine", &["owned"], false), &attempt, Some(&reported))
        .unwrap_err();
    assert!(
        error.contains(&format!(
            "reported commit {reported} is not on the shared checkout HEAD {start}"
        )),
        "{error}"
    );
    assert!(error.contains("reset or amended"), "{error}");
    assert!(matches!(
        super::report::git(
            root.path().to_str().unwrap(),
            &["merge-base", "--is-ancestor", &reported, "HEAD"],
        ),
        Err(super::report::SharedReportError::Rejected(_))
    ));
}

#[test]
fn missing_report_commit_is_a_git_failure_not_false_ancestry() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init"]);
    let error = super::report::git(
        root.path().to_str().unwrap(),
        &["merge-base", "--is-ancestor", &"f".repeat(40), "HEAD"],
    )
    .unwrap_err();
    assert!(
        matches!(error, super::report::SharedReportError::Inspection(_)),
        "{error}"
    );
    let error = error.to_string();
    assert!(error.contains("git inspection failed"), "{error}");
    assert!(!error.contains("reset or amended"), "{error}");
}

#[test]
fn cancel_before_exit_retires_once_on_confirmed_termination_and_survives_reopen() {
    use crate::plan::{
        ExecutionIntent, TaskExecutionIntent, TaskExecutorTermination, TaskLaneMetadata, TaskStatus,
    };
    use crate::service_plan::task_attempts::ExecutorSourceFields;

    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let mut plan = self::plan(root.path().to_str().unwrap());
    store.save(&mut plan, 1).unwrap();

    let ro_task = store
        .add_task(
            &plan.id,
            super::super::NewTask {
                name: "readonly".into(),
                execution_intent: TaskExecutionIntent::NonRepository,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let writer_task = store
        .add_task(
            &plan.id,
            super::super::NewTask {
                name: "writer".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let lane_task = store
        .add_task(
            &plan.id,
            super::super::NewTask {
                name: "lane".into(),
                ..Default::default()
            },
            4,
        )
        .unwrap();

    store
        .begin_task_dispatch_with_reserved_attempt_and_gates(
            &plan.id,
            &ro_task,
            "tab-ro",
            "ReadOnly",
            TaskLaneMetadata::default(),
            ExecutionIntent::ReadOnly,
            None,
            None,
            "attempt-ro",
            vec![],
            WorkerPlacement::ReadOnly,
            5,
        )
        .unwrap();
    store
        .finish_task_dispatch(
            &plan.id,
            &ro_task,
            "tab-ro",
            "attempt-ro",
            "sess-ro",
            "nonce-ro",
            TaskLaneMetadata::default(),
            6,
        )
        .unwrap();

    store
        .begin_task_dispatch_with_reserved_attempt_and_gates(
            &plan.id,
            &writer_task,
            "tab-writer",
            "Writer",
            TaskLaneMetadata::default(),
            ExecutionIntent::Mutating,
            None,
            None,
            "attempt-writer",
            vec![],
            WorkerPlacement::CanonicalWriter,
            7,
        )
        .unwrap();
    store
        .finish_task_dispatch(
            &plan.id,
            &writer_task,
            "tab-writer",
            "attempt-writer",
            "sess-writer",
            "nonce-writer",
            TaskLaneMetadata::default(),
            8,
        )
        .unwrap();
    store
        .edit_plan_fixture(&plan.id, 9, |plan| {
            let lane = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == lane_task)
                .unwrap();
            lane.lane.worktree_path = Some(root.path().join("lane-wt").display().to_string());
            let writer = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == writer_task)
                .unwrap();
            writer.status = TaskStatus::Cancelled;
            let ro = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == ro_task)
                .unwrap();
            ro.status = TaskStatus::Cancelled;
            Ok(())
        })
        .unwrap();

    assert!(
        store
            .task_lane_integration_projection(&plan.id, &lane_task)
            .unwrap()
            .is_some(),
        "status-only cancelled mutating canonical writer must still hold checkout"
    );

    let wrong = ExecutorSourceFields {
        id: "attempt-writer",
        executor_tab_id: "tab-writer",
        provider: None,
        session_id: Some("sess-writer"),
        session_binding_nonce: Some("wrong-nonce"),
        mesh_peer: None,
    };
    assert!(store
        .record_task_executor_termination(&plan.id, &writer_task, wrong, 99)
        .is_err());
    assert!(store
        .task_lane_integration_projection(&plan.id, &lane_task)
        .unwrap()
        .is_some());

    let exact_writer = ExecutorSourceFields {
        id: "attempt-writer",
        executor_tab_id: "tab-writer",
        provider: None,
        session_id: Some("sess-writer"),
        session_binding_nonce: Some("nonce-writer"),
        mesh_peer: None,
    };
    store
        .record_task_executor_termination(&plan.id, &writer_task, exact_writer, 100)
        .unwrap();
    store
        .record_task_executor_termination(&plan.id, &writer_task, exact_writer, 200)
        .unwrap();

    assert!(store
        .checkout_peer_facts(root.path().to_str().unwrap())
        .unwrap()
        .is_empty());
    assert!(store
        .task_lane_integration_projection(&plan.id, &lane_task)
        .unwrap()
        .is_none());

    drop(store);
    let reopened = PlanStore::new_for_test(root.path());
    let persisted_writer = reopened.inspect_task_for_test(&writer_task).unwrap().task;
    assert_eq!(persisted_writer.status, TaskStatus::Cancelled);
    assert_eq!(persisted_writer.assigned_tab_id, None);
    assert_eq!(persisted_writer.interruption.owner_tab_id, None);
    assert_eq!(persisted_writer.interruption.owner_session_id, None);
    assert_eq!(persisted_writer.execution_attempts.len(), 1);
    let attempt = &persisted_writer.execution_attempts[0];
    assert_eq!(attempt.closed_at, Some(100));
    assert_eq!(attempt.retired_at, Some(100));
    assert_eq!(attempt.closure_reason, None);
    assert_eq!(
        attempt.executor_termination,
        Some(TaskExecutorTermination {
            attempt_id: "attempt-writer".into(),
            executor_tab_id: "tab-writer".into(),
            session_id: Some("sess-writer".into()),
            session_binding_nonce: Some("nonce-writer".into()),
            confirmed_at: 100,
        })
    );
    assert!(reopened
        .task_lane_integration_projection(&plan.id, &lane_task)
        .unwrap()
        .is_none());
}

#[test]
fn supported_same_status_cancel_recovery_preserves_receipt_and_history_separately() {
    use crate::plan::{ExecutionIntent, TaskExecutionIntent, TaskExecutorTermination, TaskStatus};

    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let mut plan = self::plan(root.path().to_str().unwrap());
    let mut cancelled_ro = task("task_cancelled_ro", &[], false);
    cancelled_ro.status = TaskStatus::Cancelled;
    cancelled_ro.execution_intent = TaskExecutionIntent::NonRepository;
    cancelled_ro.interruption.owner_tab_id = Some("tab-gone".into());
    cancelled_ro.interruption.owner_session_id = Some("sess-gone".into());
    cancelled_ro.execution_attempts.push(TaskExecutionAttempt {
        id: "attempt_ro_hist".into(),
        executor_tab_id: "tab-gone".into(),
        session_id: Some("sess-gone".into()),
        session_binding_nonce: Some("nonce-gone".into()),
        placement: WorkerPlacement::ReadOnly,
        execution_intent: Some(ExecutionIntent::ReadOnly),
        executor_termination: Some(TaskExecutorTermination {
            attempt_id: "attempt_ro_hist".into(),
            executor_tab_id: "tab-gone".into(),
            session_id: Some("sess-gone".into()),
            session_binding_nonce: Some("nonce-gone".into()),
            confirmed_at: 1_795_216_551_003,
        }),
        ..Default::default()
    });
    let mut unrelated_lane = task("task_unrelated_lane", &["src/site.rs"], false);
    unrelated_lane.lane.worktree_path = Some(root.path().join("lane-wt").display().to_string());
    plan.tasks = vec![cancelled_ro, unrelated_lane];
    store.save(&mut plan, 1).unwrap();

    assert!(store
        .task_lane_integration_projection(&plan.id, "task_unrelated_lane")
        .unwrap()
        .is_none());

    let retirement = store
        .retire_stale_task_owner(
            &plan.id,
            "task_cancelled_ro",
            Some(TaskStatus::Cancelled),
            |_| false,
            1_795_216_781_124,
        )
        .unwrap();
    assert!(retirement.applied);
    assert_eq!(retirement.retired_attempt_ids, vec!["attempt_ro_hist"]);

    drop(store);
    let reopened = PlanStore::new_for_test(root.path());
    let persisted = reopened
        .inspect_task_for_test("task_cancelled_ro")
        .unwrap()
        .task;
    assert_eq!(persisted.status, TaskStatus::Cancelled);
    let attempt = &persisted.execution_attempts[0];
    assert_eq!(attempt.placement, WorkerPlacement::ReadOnly);
    assert_eq!(attempt.execution_intent, Some(ExecutionIntent::ReadOnly));
    assert_eq!(attempt.closed_at, Some(1_795_216_781_124));
    assert_eq!(attempt.retired_at, Some(1_795_216_781_124));
    assert_eq!(attempt.closure_reason, None);
    assert_eq!(
        attempt
            .executor_termination
            .as_ref()
            .map(|receipt| receipt.confirmed_at),
        Some(1_795_216_551_003)
    );
    assert!(reopened
        .task_lane_integration_projection(&plan.id, "task_unrelated_lane")
        .unwrap()
        .is_none());
}

#[test]
fn termination_confirmation_races_cleanly_with_review_admission_via_barrier() {
    use crate::plan::{ExecutionIntent, TaskStatus};
    use crate::service_plan::task_attempts::ExecutorSourceFields;

    let root = tempfile::tempdir().unwrap();
    let dirty_lane = root.path().join("dirty-lane");
    std::fs::create_dir_all(&dirty_lane).unwrap();
    let dirty_file = dirty_lane.join("unmerged.txt");
    std::fs::write(&dirty_file, "dirty work").unwrap();

    let store = std::sync::Arc::new(PlanStore::new_for_test(root.path()));
    let mut plan = self::plan(root.path().to_str().unwrap());
    let mut cancelled_writer = task("writer", &[], false);
    cancelled_writer.status = TaskStatus::Cancelled;
    cancelled_writer.assigned_tab_id = Some("tab-w".into());
    cancelled_writer
        .execution_attempts
        .push(TaskExecutionAttempt {
            id: "attempt-w".into(),
            executor_tab_id: "tab-w".into(),
            session_id: Some("sess-w".into()),
            session_binding_nonce: Some("nonce-w".into()),
            placement: WorkerPlacement::CanonicalWriter,
            execution_intent: Some(ExecutionIntent::Mutating),
            ..Default::default()
        });
    let mut lane_task = task("lane", &["src/a.rs"], false);
    lane_task.lane.worktree_path = Some(dirty_lane.display().to_string());
    plan.tasks = vec![cancelled_writer, lane_task];
    store.save(&mut plan, 1).unwrap();

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let confirm_store = store.clone();
    let confirm_barrier = barrier.clone();
    let confirm_plan = plan.id.clone();
    let confirmer = std::thread::spawn(move || {
        let source = ExecutorSourceFields {
            id: "attempt-w",
            executor_tab_id: "tab-w",
            provider: None,
            session_id: Some("sess-w"),
            session_binding_nonce: Some("nonce-w"),
            mesh_peer: None,
        };
        confirm_barrier.wait();
        confirm_store
            .record_task_executor_termination(&confirm_plan, "writer", source, 55)
            .unwrap();
    });

    let review_store = store.clone();
    let review_barrier = barrier.clone();
    let review_plan = plan.id.clone();
    let reviewer = std::thread::spawn(move || {
        review_barrier.wait();
        review_store
            .task_lane_integration_projection(&review_plan, "lane")
            .unwrap()
    });

    confirmer.join().unwrap();
    let observed = reviewer.join().unwrap();
    if let Some(hold) = observed {
        assert_eq!(hold.holder.task_id, "writer");
        assert_eq!(hold.holder.attempt_id, "attempt-w");
    }

    assert!(store
        .task_lane_integration_projection(&plan.id, "lane")
        .unwrap()
        .is_none());
    let persisted = store.inspect_task_for_test("writer").unwrap().task;
    assert_eq!(persisted.execution_attempts[0].closed_at, Some(55));
    assert_eq!(persisted.execution_attempts[0].retired_at, Some(55));
    assert_eq!(std::fs::read_to_string(&dirty_file).unwrap(), "dirty work");
}
