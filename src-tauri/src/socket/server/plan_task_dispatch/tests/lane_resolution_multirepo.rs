use super::super::resolve_dispatch_lane;
use crate::plan::{
    AgentRef, OrchestratorRef, Plan, PlanMode, TaskExecutionAttempt, TaskLaneMetadata, TaskStatus,
};
use crate::service::plan::{NewTask, PlanStore};
use crate::service::LaneProtection;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(path: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    crate::process_util::no_window_std(&mut command);
    let output = command.current_dir(path).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repository(container: &Path, name: &str) -> PathBuf {
    let project = container.join(name);
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q"]);
    git(&project, &["config", "user.email", "test@example.com"]);
    git(&project, &["config", "user.name", "Test"]);
    std::fs::write(project.join("README.md"), name).unwrap();
    git(&project, &["add", "README.md"]);
    git(&project, &["commit", "-qm", "fixture"]);
    project
}

fn assert_resolves_to(resolved: &str, expected: &Path) {
    assert_eq!(
        std::fs::canonicalize(resolved).unwrap(),
        std::fs::canonicalize(expected).unwrap()
    );
}

#[test]
fn lane_resolution_multirepo_explicit_repo_beats_nonrepo_workspace() {
    let container = tempfile::tempdir().unwrap();
    let backend = repository(container.path(), "backend");

    let resolved = resolve_dispatch_lane(
        Some(backend.to_str().unwrap()),
        Some(container.path().to_str().unwrap()),
        None,
        &TaskLaneMetadata::default(),
        LaneProtection::Enforced,
    )
    .unwrap();

    assert_resolves_to(&resolved.spawn_cwd, &backend);
}

#[test]
fn lane_resolution_multirepo_honours_each_address_before_the_next() {
    let container = tempfile::tempdir().unwrap();
    let recorded_repo = repository(container.path(), "recorded");
    let explicit_repo = repository(container.path(), "explicit");
    let workspace_repo = repository(container.path(), "workspace");
    let caller_repo = repository(container.path(), "caller");
    let linked = container.path().join("linked");
    git(
        &recorded_repo,
        &[
            "worktree",
            "add",
            "-b",
            "lane/recorded",
            linked.to_str().unwrap(),
        ],
    );

    let recorded = resolve_dispatch_lane(
        Some(explicit_repo.to_str().unwrap()),
        Some(workspace_repo.to_str().unwrap()),
        Some(caller_repo.to_str().unwrap()),
        &TaskLaneMetadata {
            worktree_path: Some(linked.display().to_string()),
            branch: Some("lane/recorded".into()),
            ..Default::default()
        },
        LaneProtection::Enforced,
    )
    .unwrap();
    assert_resolves_to(&recorded.spawn_cwd, &linked);

    let explicit = resolve_dispatch_lane(
        Some(explicit_repo.to_str().unwrap()),
        Some(workspace_repo.to_str().unwrap()),
        Some(caller_repo.to_str().unwrap()),
        &TaskLaneMetadata::default(),
        LaneProtection::Enforced,
    )
    .unwrap();
    assert_resolves_to(&explicit.spawn_cwd, &explicit_repo);

    let workspace = resolve_dispatch_lane(
        None,
        Some(workspace_repo.to_str().unwrap()),
        Some(caller_repo.to_str().unwrap()),
        &TaskLaneMetadata::default(),
        LaneProtection::Enforced,
    )
    .unwrap();
    assert_resolves_to(&workspace.spawn_cwd, &workspace_repo);

    let caller = resolve_dispatch_lane(
        None,
        Some(container.path().to_str().unwrap()),
        Some(caller_repo.to_str().unwrap()),
        &TaskLaneMetadata::default(),
        LaneProtection::Enforced,
    )
    .unwrap();
    assert_resolves_to(&caller.spawn_cwd, &caller_repo);

    git(
        &recorded_repo,
        &["worktree", "remove", "--force", linked.to_str().unwrap()],
    );
}

#[test]
fn lane_resolution_multirepo_accepts_a_recorded_linked_worktree() {
    let container = tempfile::tempdir().unwrap();
    let project = repository(container.path(), "backend");
    let linked = container.path().join("lanes").join("worker");
    std::fs::create_dir_all(linked.parent().unwrap()).unwrap();
    git(
        &project,
        &[
            "worktree",
            "add",
            "-b",
            "lane/linked",
            linked.to_str().unwrap(),
        ],
    );
    assert!(linked.join(".git").is_file());

    let resolved = resolve_dispatch_lane(
        None,
        Some(container.path().to_str().unwrap()),
        None,
        &TaskLaneMetadata {
            worktree_path: Some(linked.display().to_string()),
            branch: Some("lane/linked".into()),
            ..Default::default()
        },
        LaneProtection::Enforced,
    )
    .unwrap();

    assert_resolves_to(&resolved.spawn_cwd, &linked);
    git(
        &project,
        &["worktree", "remove", "--force", linked.to_str().unwrap()],
    );
}

#[test]
fn reopened_task_redispatches_onto_its_lane() {
    let container = tempfile::tempdir().unwrap();
    let project = repository(container.path(), "reopened");
    let lane_path = container.path().join("lanes").join("worker");
    std::fs::create_dir_all(lane_path.parent().unwrap()).unwrap();
    git(
        &project,
        &[
            "worktree",
            "add",
            "-b",
            "lane/reopened",
            lane_path.to_str().unwrap(),
        ],
    );
    let provenance = resolve_dispatch_lane(
        Some(lane_path.to_str().unwrap()),
        Some(project.to_str().unwrap()),
        None,
        &TaskLaneMetadata::default(),
        LaneProtection::Enforced,
    )
    .unwrap()
    .existing
    .unwrap();
    let lane = super::super::lane_metadata_for_dispatch(
        &TaskLaneMetadata {
            tab_id: Some("tab_gone".into()),
            ..Default::default()
        },
        &provenance,
        project.to_str().unwrap(),
    );
    let store = PlanStore::new_for_test(container.path());
    let mut plan = Plan::new(
        "Reopened lane".into(),
        project.to_string_lossy().into_owned(),
        OrchestratorRef::from_tab("orchestrator"),
    );
    plan.mode = PlanMode::Execution;
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Retry reviewed work".into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["keep the recorded lane".into()],
                test_plan: Some("Verify the reopened dispatch path".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .with_plan(&plan.id, 3, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.status = TaskStatus::InReview;
            task.assigned_tab_id = Some("tab_gone".into());
            task.lane = lane.clone();
            task.execution_attempts.push(TaskExecutionAttempt {
                actual_reasoning_effort: None,
                id: "attempt_gone".into(),
                executor_tab_id: "tab_gone".into(),
                session_id: Some("session_gone".into()),
                acknowledged_at: Some(3),
                reported_at: Some(3),
                report: Some(Default::default()),
                closed_at: Some(3),
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();
    store
        .bounce_review(
            &plan.id,
            &task_id,
            "retry the same lane",
            false,
            false,
            AgentRef::from_tab("orchestrator"),
            4,
        )
        .unwrap();

    let reopened_plan = store.get(&plan.id).unwrap();
    let reopened_task = reopened_plan
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .unwrap();
    assert_eq!(reopened_task.status, TaskStatus::Pending);
    let resolved = resolve_dispatch_lane(
        None,
        Some(project.to_str().unwrap()),
        None,
        &reopened_task.lane,
        LaneProtection::Enforced,
    )
    .unwrap();
    let existing = resolved
        .existing
        .expect("redispatch must resolve the task's recorded worktree");
    let placement = super::super::plan_dispatch_lane_placement(
        &reopened_plan,
        &task_id,
        &reopened_task.lane,
        existing.clone(),
    )
    .unwrap();
    assert!(matches!(
        placement,
        super::super::DispatchLanePlacement::Existing(_)
    ));

    let attempt_id = store
        .begin_task_dispatch(
            &plan.id,
            &task_id,
            "tab_replacement",
            "Worker",
            super::super::lane_metadata_for_dispatch(
                &reopened_task.lane,
                &existing,
                project.to_str().unwrap(),
            ),
            5,
        )
        .unwrap();
    let redispatched = store.show_task(&task_id).unwrap().task;
    assert_eq!(redispatched.lane.tab_id.as_deref(), Some("tab_replacement"));
    assert_eq!(redispatched.lane.worktree_path, lane.worktree_path);
    assert_eq!(redispatched.lane.branch, lane.branch);
    assert_eq!(
        redispatched
            .execution_attempts
            .iter()
            .find(|attempt| attempt.id == attempt_id)
            .and_then(|attempt| attempt.worktree_path.as_deref()),
        lane.worktree_path.as_deref()
    );
}

#[test]
fn lane_resolution_multirepo_keeps_single_repo_workspace_resolution() {
    let container = tempfile::tempdir().unwrap();
    let project = repository(container.path(), "project");

    let resolved = resolve_dispatch_lane(
        None,
        Some(project.to_str().unwrap()),
        None,
        &TaskLaneMetadata::default(),
        LaneProtection::Enforced,
    )
    .unwrap();

    assert_resolves_to(&resolved.spawn_cwd, &project);
}

#[test]
fn lane_resolution_multirepo_names_every_unresolvable_address() {
    let container = tempfile::tempdir().unwrap();
    let recorded = container.path().join("missing-recorded");
    let explicit = container.path().join("missing-explicit");
    let workspace = container.path().join("missing-workspace");
    let caller = container.path().join("missing-caller");
    let error = resolve_dispatch_lane(
        Some(explicit.to_str().unwrap()),
        Some(workspace.to_str().unwrap()),
        Some(caller.to_str().unwrap()),
        &TaskLaneMetadata {
            worktree_path: Some(recorded.display().to_string()),
            ..Default::default()
        },
        LaneProtection::Enforced,
    )
    .unwrap_err();
    let message = error.message;
    let positions = [
        recorded.to_str().unwrap(),
        explicit.to_str().unwrap(),
        workspace.to_str().unwrap(),
        caller.to_str().unwrap(),
    ]
    .map(|path| message.find(path).unwrap());
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    // A generic refusal once replaced every candidate's own reason, so the
    // failure pointed at checkout resolution instead of the real condition.
    for path in [
        recorded.to_str().unwrap(),
        explicit.to_str().unwrap(),
        workspace.to_str().unwrap(),
        caller.to_str().unwrap(),
    ] {
        assert!(
            message.contains(&format!("'{path}': cannot resolve worker lane path")),
            "{message}"
        );
    }
}
