use super::*;
use crate::plan::{OrchestratorRef, PlanMode, TaskLaneMetadata, TaskStatus};
use crate::service::plan::task_attempts::TaskRespawnRequest;
use crate::service::plan::{NewTask, PlanStore};

#[test]
fn respawn_git_lane_decision_preserves_recorded_lanes() {
    for (intent, task_intent, requires_git) in [
        (
            ExecutionIntent::ReadOnly,
            TaskExecutionIntent::NonRepository,
            false,
        ),
        (
            ExecutionIntent::ReadOnly,
            TaskExecutionIntent::RepositoryMutation,
            false,
        ),
        (
            ExecutionIntent::Mutating,
            TaskExecutionIntent::NonRepository,
            false,
        ),
        (
            ExecutionIntent::Mutating,
            TaskExecutionIntent::RepositoryMutation,
            true,
        ),
        (
            ExecutionIntent::Unspecified,
            TaskExecutionIntent::NonRepository,
            false,
        ),
        (
            ExecutionIntent::Unspecified,
            TaskExecutionIntent::RepositoryMutation,
            true,
        ),
    ] {
        assert_eq!(
            respawn_requires_git_lane(&intent, task_intent, None, None),
            requires_git,
        );
        for (task_path, attempt_path) in [
            (Some("/lane"), None),
            (None, Some("/attempt-lane")),
            (Some("/lane"), Some("/attempt-lane")),
            (Some(""), None),
        ] {
            assert!(respawn_requires_git_lane(
                &intent,
                task_intent,
                task_path,
                attempt_path,
            ));
        }
    }
}

fn respawn_fixture(
    store: &PlanStore,
    project: &str,
    task_intent: TaskExecutionIntent,
    intent: ExecutionIntent,
) -> (String, Task, TaskRespawnDraft) {
    let plan = store
        .create_plan_for_team(
            "respawn".into(),
            project.into(),
            OrchestratorRef::from_tab("orch"),
            "team-respawn".into(),
            vec![],
            1,
        )
        .unwrap();
    let mut approved = store.get(&plan.id).unwrap();
    approved.mode = PlanMode::Approved;
    store.save(&mut approved, 2).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "worker".into(),
                execution_intent: task_intent,
                ..Default::default()
            },
            3,
        )
        .unwrap();
    store
        .with_plan(&plan.id, 4, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, &task_id).unwrap();
            task.interruption.execution_intent = intent;
            Ok(())
        })
        .unwrap();
    let source_id = store
        .begin_task_dispatch(
            &plan.id,
            &task_id,
            "tab-first",
            "Ada",
            TaskLaneMetadata::default(),
            5,
        )
        .unwrap();
    let draft = store
        .build_task_respawn_draft(
            &plan.id,
            &task_id,
            TaskRespawnRequest {
                attempt_id: source_id,
                ..Default::default()
            },
        )
        .unwrap();
    (plan.id, store.show_task(&task_id).unwrap().task, draft)
}

#[test]
fn non_repository_respawn_in_non_git_directory_persists_acknowledged_replacement() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("non-git");
    std::fs::create_dir(&project).unwrap();
    // macOS tempdirs live under the /var -> /private/var symlink; respawn records the canonical cwd.
    let project = project.canonicalize().unwrap();
    let project = project.to_str().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let (plan_id, task, draft) = respawn_fixture(
        &store,
        project,
        TaskExecutionIntent::NonRepository,
        ExecutionIntent::ReadOnly,
    );
    assert!(!std::path::Path::new(project).join(".git").exists());
    assert!(task.lane.worktree_path.is_none());
    assert!(draft.source_attempt.worktree_path.is_none());
    assert_eq!(draft.cwd, project);
    assert!(
        verify_respawn_lane("/absent-canonical-checkout", &task, &draft)
            .unwrap()
            .is_none()
    );
    let created = store
        .record_task_respawn_attempt(
            &plan_id,
            &task.id,
            "tab-replacement",
            &draft,
            Some("worker"),
            6,
        )
        .unwrap();
    store
        .finish_task_dispatch(
            &plan_id,
            &task.id,
            "tab-replacement",
            &created.attempt.id,
            "session-replacement",
            "nonce-replacement",
            task.lane,
            7,
        )
        .unwrap();
    store
        .acknowledge_live_task_attempt_for_tab("tab-replacement", None, None, 8)
        .unwrap();
    let persisted = store.show_task(&task.id).unwrap().task;
    assert_eq!(persisted.status, TaskStatus::InProgress);
    assert_eq!(
        persisted.assigned_tab_id.as_deref(),
        Some("tab-replacement")
    );
    assert!(persisted.lane.worktree_path.is_none());
    let source = persisted
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == draft.source_attempt.id)
        .unwrap();
    assert!(source.closed_at.is_some());
    let replacement = persisted
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == draft.attempt_id)
        .unwrap();
    assert_eq!(replacement.acknowledged_at, Some(8));
    assert!(replacement.worktree_path.is_none());
}

#[test]
fn lane_free_respawn_checks_cwd_and_source_attempt() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().to_str().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let (_, task, mut draft) = respawn_fixture(
        &store,
        project,
        TaskExecutionIntent::NonRepository,
        ExecutionIntent::Mutating,
    );
    assert!(verify_respawn_lane(project, &task, &draft)
        .unwrap()
        .is_none());
    draft.cwd = root.path().join("missing").display().to_string();
    assert!(verify_respawn_lane(project, &task, &draft)
        .unwrap_err()
        .contains("cwd"));
    let file = root.path().join("file");
    std::fs::write(&file, "file").unwrap();
    draft.cwd = file.display().to_string();
    assert!(verify_respawn_lane(project, &task, &draft)
        .unwrap_err()
        .contains("cwd"));
    draft.cwd = project.into();
    draft.source_attempt.id = "foreign-attempt".into();
    assert!(verify_respawn_lane(project, &task, &draft)
        .unwrap_err()
        .contains("source attempt"));
}

#[test]
fn repository_respawn_and_recorded_read_only_lane_still_inspect_git() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().to_str().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let (_, mut task, mut draft) = respawn_fixture(
        &store,
        project,
        TaskExecutionIntent::RepositoryMutation,
        ExecutionIntent::Mutating,
    );
    assert!(verify_respawn_lane(project, &task, &draft)
        .unwrap_err()
        .contains("failed to inspect worker lane"));
    task.execution_intent = TaskExecutionIntent::NonRepository;
    task.interruption.execution_intent = ExecutionIntent::ReadOnly;
    task.lane.worktree_path = Some(project.into());
    assert!(verify_respawn_lane(project, &task, &draft)
        .unwrap_err()
        .contains("failed to inspect worker lane"));
    task.lane.worktree_path = None;
    draft.source_attempt.worktree_path = Some(project.into());
    assert!(verify_respawn_lane(project, &task, &draft)
        .unwrap_err()
        .contains("failed to inspect worker lane"));
}
