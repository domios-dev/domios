//! The dispatcher's placement decision and the two payloads that report it.
//! Every assertion compares values the test itself supplied or that git
//! produced, never prose: the defects here were a refusal where a lane should
//! have been provisioned, and two payloads disagreeing about which checkout is
//! canonical.

use crate::plan::{
    AgentRef, ExecutionIntent, OrchestratorRef, Plan, TaskExecutionAttempt, TaskExecutionIntent,
    TaskLaneMetadata, TaskStatus,
};
use crate::service::plan::{NewTask, PlanStore, TaskLaneUpdate};
use crate::service::{
    provision_lane_for_test, refuse_unsafe_task_bound_mutating_lane, LaneProtection,
    ProvisionedWorkerLane, WorkerLaneProvenance,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::super::lane_safety::{
    atomic_lane_commitment, refuse_unsafe_mutating_write, REFUSAL_MUTATING_WRITE_IN_CANONICAL,
    REFUSAL_MUTATING_WRITE_IN_FOREIGN_LANE,
};
use super::super::{
    dispatch_ready_event, dispatch_response_lane, lane_metadata_for_dispatch,
    plan_dispatch_lane_placement, resolve_dispatch_lane, DispatchEvidence, DispatchLanePlacement,
};

struct Fixture {
    root: tempfile::TempDir,
    store: PlanStore,
    plan: Plan,
    project: PathBuf,
    mine: String,
    theirs: String,
}

fn violation_id(error: &crate::socket::server::JsonRpcError) -> String {
    error.data.as_ref().unwrap()["violation_id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// A plan whose canonical project is a real repository, plus a second task to
/// stand in for the foreign-lane owner.
fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    super::git(&project, &["init", "-q"]);
    super::git(&project, &["config", "user.email", "test@example.com"]);
    super::git(&project, &["config", "user.name", "Test"]);
    std::fs::write(project.join("README.md"), "fixture").unwrap();
    super::git(&project, &["add", "README.md"]);
    super::git(&project, &["commit", "-qm", "fixture"]);

    let store = PlanStore::new_for_test(root.path());
    let created = store
        .create_plan_for_team(
            "Lane provisioning".into(),
            project.display().to_string(),
            OrchestratorRef::from_tab("orch"),
            "team".into(),
            vec![],
            1,
        )
        .unwrap();
    let mine = store
        .add_task(
            &created.id,
            NewTask {
                name: "mine".into(),
                execution_intent: TaskExecutionIntent::RepositoryMutation,
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let theirs = store
        .add_task(
            &created.id,
            NewTask {
                name: "theirs".into(),
                execution_intent: TaskExecutionIntent::RepositoryMutation,
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let plan = store.get(&created.id).unwrap();
    Fixture {
        root,
        store,
        plan,
        project,
        mine,
        theirs,
    }
}

#[test]
fn terminal_cleanup_clears_removed_lane_before_the_next_dispatch() {
    let fixture = fixture();
    let first_lane = fixture.lane_for(&fixture.mine);
    let first = first_lane.verify().expect("the initial lane verifies");
    fixture
        .store
        .update_task_lane(
            &fixture.plan.id,
            &fixture.mine,
            TaskLaneUpdate {
                tab_id: Some(Some("tab-worker".into())),
                workspace_id: Some(Some("ws_old".into())),
                workspace_name: Some(Some("old workspace".into())),
                worktree_path: Some(Some(first.worktree_path.clone())),
                branch: Some(Some(first.branch.clone())),
                base: Some(Some(first.base.clone())),
                destination: Some(Some(first.base.clone())),
                ..Default::default()
            },
            4,
        )
        .unwrap();
    fixture
        .store
        .with_plan(&fixture.plan.id, 5, |plan| {
            plan.tasks
                .iter_mut()
                .find(|task| task.id == fixture.mine)
                .expect("fixture task")
                .execution_attempts
                .push(TaskExecutionAttempt {
                    actual_reasoning_effort: None,
                    id: "attempt-preserved".into(),
                    executor_tab_id: "tab-worker".into(),
                    branch: Some(first.branch.clone()),
                    worktree_path: Some(first.worktree_path.clone()),
                    assigned_at: Some(2),
                    acknowledged_at: Some(3),
                    reported_at: Some(5),
                    ..Default::default()
                });
            Ok(())
        })
        .unwrap();
    fixture
        .store
        .close_task(
            &fixture.plan.id,
            &fixture.mine,
            Some("finished"),
            AgentRef::from_tab("orch"),
            6,
        )
        .unwrap();

    let terminal_plan = fixture.store.get(&fixture.plan.id).unwrap();
    let outcome = crate::service::plan::lane_cleanup::try_cleanup_task_lane(
        &fixture.store,
        &terminal_plan,
        &fixture.mine,
        |_| false,
        7,
    );

    assert!(matches!(
        outcome,
        crate::service::plan::lane_cleanup::LaneCleanupOutcome::Cleaned
    ));
    assert!(!Path::new(&first.worktree_path).exists());
    assert!(!branch_exists(&fixture.project, &first.branch));

    let cleaned = fixture.store.show_task(&fixture.mine).unwrap().task;
    assert_eq!(cleaned.status, TaskStatus::Done);
    assert!(cleaned.assigned_tab_id.is_none());
    assert!(cleaned.lane.tab_id.is_none());
    assert!(cleaned.lane.workspace_id.is_none());
    assert!(cleaned.lane.workspace_name.is_none());
    assert!(cleaned.lane.worktree_path.is_none());
    assert!(cleaned.lane.branch.is_none());
    assert_eq!(cleaned.lane.base.as_deref(), Some(first.base.as_str()));
    assert_eq!(
        cleaned.lane.destination.as_deref(),
        Some(first.base.as_str())
    );
    assert_eq!(cleaned.lane.cleanup_status.as_deref(), Some("done"));
    let attempt = &cleaned.execution_attempts[0];
    assert_eq!(
        attempt.worktree_path.as_deref(),
        Some(first.worktree_path.as_str())
    );
    assert_eq!(attempt.branch.as_deref(), Some(first.branch.as_str()));
    assert_eq!(attempt.assigned_at, Some(2));
    assert_eq!(attempt.acknowledged_at, Some(3));
    assert_eq!(attempt.reported_at, Some(5));
    assert_eq!(attempt.closed_at, Some(6));

    let repeated = crate::service::plan::lane_cleanup::try_cleanup_task_lane(
        &fixture.store,
        &fixture.store.get(&fixture.plan.id).unwrap(),
        &fixture.mine,
        |_| false,
        8,
    );
    assert_eq!(
        repeated,
        crate::service::plan::lane_cleanup::LaneCleanupOutcome::Noop
    );

    fixture
        .store
        .update_task(
            &fixture.plan.id,
            &fixture.mine,
            None,
            None,
            None,
            None,
            Some("pending"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            9,
        )
        .unwrap();
    let pending_plan = fixture.store.get(&fixture.plan.id).unwrap();
    let pending_task = fixture.store.show_task(&fixture.mine).unwrap().task;
    let resolved = resolved_from_workspace(&fixture.project, &pending_task.lane);
    let placement = plan_dispatch_lane_placement(
        &pending_plan,
        pending_task.id.as_str(),
        &pending_task.lane,
        resolved,
    )
    .expect("the reopened task receives a freshly provisioned isolated lane");
    let DispatchLanePlacement::Provision { base_checkout } = placement else {
        panic!("expected fresh lane provisioning, got {placement:?}");
    };
    let next_lane = provision_lane_for_test(&base_checkout, "after-terminal-cleanup").unwrap();
    let next = next_lane.verify().expect("the next isolated lane verifies");
    assert_ne!(next.worktree_path, first.worktree_path);
    assert_ne!(next.branch, first.branch);
    assert!(!same_dir(&next.worktree_path, &fixture.project));
}

impl Fixture {
    fn task(&self, task_id: &str) -> crate::plan::Task {
        self.plan
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .expect("fixture task")
            .clone()
    }

    fn record_lane(&mut self, task_id: &str, worktree: &str) {
        self.plan
            .tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .expect("fixture task")
            .lane
            .worktree_path = Some(worktree.to_string());
    }

    fn lane_for(&self, task_id: &str) -> ProvisionedWorkerLane {
        provision_lane_for_test(
            self.project.to_str().unwrap(),
            &format!("{}-{}", self.plan.id, task_id),
        )
        .expect("a sibling lane provisions from the canonical project")
    }
}

fn resolved_from_workspace(project: &Path, lane: &TaskLaneMetadata) -> WorkerLaneProvenance {
    resolve_dispatch_lane(
        None,
        Some(project.to_str().unwrap()),
        None,
        lane,
        LaneProtection::Enforced,
    )
    .unwrap()
    .existing
    .expect("a repository checkout resolves")
}

fn same_dir(a: &str, b: &Path) -> bool {
    std::fs::canonicalize(a).unwrap() == std::fs::canonicalize(b).unwrap()
}

fn branch_exists(project: &Path, branch: &str) -> bool {
    let reference = format!("refs/heads/{branch}");
    let mut command = Command::new("git");
    crate::process_util::no_window_std(&mut command);
    command
        .current_dir(project)
        .args(["show-ref", "--verify", "--quiet", &reference])
        .status()
        .expect("git runs")
        .success()
}

fn recorded(worktree: &str) -> TaskLaneMetadata {
    TaskLaneMetadata {
        worktree_path: Some(worktree.to_string()),
        ..Default::default()
    }
}

#[test]
fn dispatch_provisions_unrecorded_lane() {
    let fixture = fixture();
    let resolved = resolved_from_workspace(&fixture.project, &TaskLaneMetadata::default());
    // The precondition the fix exists for: with no recorded lane the dispatch
    // resolves the canonical checkout, and used to be refused there.
    assert!(same_dir(&resolved.worktree_path, &fixture.project));

    let placement = plan_dispatch_lane_placement(
        &fixture.plan,
        &fixture.mine,
        &TaskLaneMetadata::default(),
        resolved,
    )
    .expect("an unrecorded mutating dispatch provisions instead of refusing");
    let DispatchLanePlacement::Provision { base_checkout } = placement else {
        panic!("expected provisioning, got {placement:?}");
    };
    assert!(same_dir(&base_checkout, &fixture.project));

    let lane = fixture.lane_for(&fixture.mine);
    let provenance = lane.verify().expect("the provisioned lane verifies");

    // One sibling lane, outside canonical, on its own branch.
    assert!(!same_dir(&provenance.worktree_path, &fixture.project));
    assert!(same_dir(&provenance.canonical_cwd, &fixture.project));
    assert!(!provenance.branch.is_empty());
    assert_eq!(
        Path::new(&provenance.worktree_path)
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str()),
        Some("project-worktrees"),
        "the lane must live in the canonical project's sibling container"
    );

    // The same hard stop the red line names, asked of the lane that was built.
    refuse_unsafe_mutating_write(
        &fixture.plan,
        &fixture.mine,
        &provenance.worktree_path,
        ExecutionIntent::Mutating,
    )
    .expect("a freshly provisioned lane is this task's own");

    // Provenance and Team identity commit as one value.
    let with_identity = TaskLaneMetadata {
        workspace_id: Some("ws_1".into()),
        workspace_name: Some("codeterm".into()),
        ..Default::default()
    };
    let committed = atomic_lane_commitment(&lane_metadata_for_dispatch(
        &with_identity,
        &provenance,
        &fixture.project.display().to_string(),
    ));
    assert_eq!(
        committed.worktree_path.as_deref(),
        Some(provenance.worktree_path.as_str())
    );
    assert_eq!(
        committed.branch.as_deref(),
        Some(provenance.branch.as_str())
    );
    assert_eq!(committed.base.as_deref(), Some(provenance.base.as_str()));
    assert_eq!(committed.workspace_id.as_deref(), Some("ws_1"));
    assert_eq!(committed.workspace_name.as_deref(), Some("codeterm"));

    lane.cleanup().unwrap();
}

#[test]
fn dispatch_refuses_a_recorded_canonical_lane_before_provisioning() {
    let mut fixture = fixture();
    let project = fixture.project.display().to_string();
    let mine = fixture.mine.clone();
    fixture.record_lane(&mine, &project);
    let lane = recorded(&project);
    let resolved = resolved_from_workspace(&fixture.project, &lane);

    let error = plan_dispatch_lane_placement(&fixture.plan, &mine, &lane, resolved).unwrap_err();

    // No placement value exists, so nothing was provisioned and no tab created.
    assert_eq!(violation_id(&error), REFUSAL_MUTATING_WRITE_IN_CANONICAL);
    assert_eq!(error.data.as_ref().unwrap()["stage"], "lane_safety");
    assert!(!error.data.as_ref().unwrap()["safe_alternative"]
        .as_str()
        .unwrap()
        .is_empty());
}

#[test]
fn dispatch_refuses_a_recorded_foreign_lane_before_provisioning() {
    let mut fixture = fixture();
    let theirs = fixture.theirs.clone();
    let mine = fixture.mine.clone();
    let foreign = fixture.lane_for(&theirs);
    let foreign_path = foreign.verify().unwrap().worktree_path;
    fixture.record_lane(&theirs, &foreign_path);
    fixture.record_lane(&mine, &foreign_path);
    let lane = recorded(&foreign_path);
    let resolved = resolved_from_workspace(&fixture.project, &lane);
    assert!(same_dir(&resolved.worktree_path, Path::new(&foreign_path)));

    let error = plan_dispatch_lane_placement(&fixture.plan, &mine, &lane, resolved).unwrap_err();

    assert_eq!(violation_id(&error), REFUSAL_MUTATING_WRITE_IN_FOREIGN_LANE);
    assert!(error.message.contains(&theirs), "{}", error.message);
    foreign.cleanup().unwrap();
}

#[test]
fn vanished_recorded_lane_is_cleared_and_a_fresh_lane_is_provisioned() {
    let fixture = fixture();
    let old_lane = fixture.lane_for(&fixture.mine);
    let old = old_lane.verify().expect("old lane verifies");
    fixture
        .store
        .update_task_lane(
            &fixture.plan.id,
            &fixture.mine,
            TaskLaneUpdate {
                tab_id: Some(Some("old-tab".into())),
                workspace_id: Some(Some("old-workspace".into())),
                workspace_name: Some(Some("old workspace".into())),
                worktree_path: Some(Some(old.worktree_path.clone())),
                branch: Some(Some(old.branch.clone())),
                base: Some(Some(old.base.clone())),
                destination: Some(Some(old.base.clone())),
                ..Default::default()
            },
            4,
        )
        .unwrap();
    fixture
        .store
        .with_plan(&fixture.plan.id, 5, |plan| {
            plan.tasks
                .iter_mut()
                .find(|task| task.id == fixture.mine)
                .expect("fixture task")
                .execution_attempts
                .push(TaskExecutionAttempt {
                    actual_reasoning_effort: None,
                    id: "old-attempt".into(),
                    executor_tab_id: "old-tab".into(),
                    branch: Some(old.branch.clone()),
                    worktree_path: Some(old.worktree_path.clone()),
                    assigned_at: Some(4),
                    ..Default::default()
                });
            Ok(())
        })
        .unwrap();
    std::fs::remove_dir_all(&old.worktree_path).unwrap();
    super::git(&fixture.project, &["worktree", "prune", "--expire", "now"]);
    super::git(&fixture.project, &["branch", "-D", &old.branch]);

    let plan = fixture.store.get(&fixture.plan.id).unwrap();
    let recorded_task = fixture.store.show_task(&fixture.mine).unwrap().task;
    assert_eq!(recorded_task.assigned_tab_id.as_deref(), Some("old-tab"));
    assert_eq!(
        recorded_task.lane.worktree_path.as_deref(),
        Some(old.worktree_path.as_str())
    );
    assert_eq!(
        recorded_task.lane.branch.as_deref(),
        Some(old.branch.as_str())
    );
    assert_eq!(recorded_task.execution_attempts.len(), 1);
    assert_eq!(
        recorded_task.execution_attempts[0].worktree_path.as_deref(),
        Some(old.worktree_path.as_str())
    );
    assert_eq!(
        recorded_task.execution_attempts[0].branch.as_deref(),
        Some(old.branch.as_str())
    );

    let resolved = resolved_from_workspace(&fixture.project, &recorded_task.lane);
    let placement =
        plan_dispatch_lane_placement(&plan, &fixture.mine, &recorded_task.lane, resolved)
            .expect("dispatch resolves to a fresh lane placement");
    let DispatchLanePlacement::Provision { base_checkout } = placement else {
        panic!("expected fresh lane provisioning, got {placement:?}");
    };
    assert!(same_dir(&base_checkout, &fixture.project));

    let mut stale_route = recorded_task.lane.clone();
    super::super::lane_commit::clear_replaced_lane_routing_fields(&mut stale_route);
    assert!(stale_route.tab_id.is_none());
    assert!(stale_route.workspace_id.is_none());
    assert!(stale_route.workspace_name.is_none());
    assert!(stale_route.worktree_path.is_none());
    assert!(stale_route.branch.is_none());
    let fresh = provision_lane_for_test(&base_checkout, "fresh-after-vanish").unwrap();
    let provenance = fresh.verify().unwrap();
    let starting_lane = atomic_lane_commitment(&lane_metadata_for_dispatch(
        &stale_route,
        &provenance,
        &fixture.project.display().to_string(),
    ));
    fixture
        .store
        .begin_task_dispatch_with_policy(
            &fixture.plan.id,
            &fixture.mine,
            "fresh-tab",
            "Worker",
            starting_lane,
            ExecutionIntent::Mutating,
            None,
            Some("codex"),
            6,
        )
        .unwrap();
    let committed = fixture.store.show_task(&fixture.mine).unwrap().task;
    assert_eq!(committed.assigned_tab_id.as_deref(), Some("fresh-tab"));
    assert_eq!(
        committed.lane.worktree_path.as_deref(),
        Some(provenance.worktree_path.as_str())
    );
    assert_eq!(
        committed.lane.branch.as_deref(),
        Some(provenance.branch.as_str())
    );
    assert_eq!(committed.execution_attempts.len(), 2);
    assert_eq!(
        committed.execution_attempts[0].worktree_path.as_deref(),
        Some(old.worktree_path.as_str())
    );
    fresh.cleanup().unwrap();
}

#[test]
fn recorded_lane_with_surviving_branch_commits_still_refuses() {
    let fixture = fixture();
    let old_lane = fixture.lane_for(&fixture.mine);
    let old = old_lane.verify().expect("old lane verifies");
    std::fs::write(
        Path::new(&old.worktree_path).join("work.txt"),
        "unmerged work",
    )
    .unwrap();
    super::git(Path::new(&old.worktree_path), &["add", "work.txt"]);
    super::git(
        Path::new(&old.worktree_path),
        &["commit", "-qm", "unmerged work"],
    );
    std::fs::remove_dir_all(&old.worktree_path).unwrap();
    super::git(&fixture.project, &["worktree", "prune", "--expire", "now"]);
    let lane = TaskLaneMetadata {
        worktree_path: Some(old.worktree_path.clone()),
        branch: Some(old.branch.clone()),
        base: Some(old.base.clone()),
        ..Default::default()
    };
    let resolved = resolved_from_workspace(&fixture.project, &TaskLaneMetadata::default());

    let error = plan_dispatch_lane_placement(&fixture.plan, &fixture.mine, &lane, resolved)
        .expect_err("a surviving branch must prevent provisioning over possible work");

    assert!(error.message.contains(&old.branch), "{}", error.message);
    assert!(error.message.contains("branch '") && error.message.contains("commits not merged"));
    assert!(error.message.contains("codeterm plan lane-remove"));
    assert!(error.message.contains("--project"));
    assert!(!error.message.contains("branch -D"));
    super::git(&fixture.project, &["branch", "-D", &old.branch]);
}

#[test]
fn recorded_lane_with_a_surviving_worktree_registration_still_refuses() {
    let fixture = fixture();
    // macOS temp dirs sit behind the /var symlink, and git records the resolved path.
    let root = if cfg!(unix) {
        fixture.root.path().canonicalize().unwrap()
    } else {
        fixture.root.path().to_path_buf()
    };
    let registered_path = root.join("registered-lane");
    super::git(
        &fixture.project,
        &[
            "worktree",
            "add",
            "--detach",
            "-q",
            registered_path.to_str().unwrap(),
            "HEAD",
        ],
    );
    std::fs::remove_dir_all(&registered_path).unwrap();
    let lane = TaskLaneMetadata {
        worktree_path: Some(registered_path.display().to_string()),
        branch: Some("lane/already-gone".into()),
        ..Default::default()
    };
    let resolved = resolved_from_workspace(&fixture.project, &TaskLaneMetadata::default());

    let error = plan_dispatch_lane_placement(&fixture.plan, &fixture.mine, &lane, resolved)
        .expect_err("a stale registration still protects its recorded lane");

    assert!(error.message.contains("Git worktree registration"));
    assert!(error
        .message
        .contains(&registered_path.display().to_string()));
    assert!(error.message.contains("codeterm plan lane-remove"));
    assert!(error.message.contains("--project"));
    super::git(&fixture.project, &["worktree", "prune", "--expire", "now"]);
}

#[test]
fn task_bound_mutating_spawn_rejects_missing_lane() {
    let mut fixture = fixture();
    let mine = fixture.task(&fixture.mine);
    let canonical = fixture.project.display().to_string();
    let mutating = Some(&ExecutionIntent::Mutating);

    // No checkout at all: the tab would inherit the daemon's working directory.
    let unnamed =
        refuse_unsafe_task_bound_mutating_lane(&fixture.plan, &mine, mutating, None).unwrap_err();
    assert!(unnamed.contains(&fixture.mine), "{unnamed}");
    assert!(unnamed.contains("plan task dispatch"), "{unnamed}");

    // Canonical: the exact hole the spawn routes left open.
    let in_canonical =
        refuse_unsafe_task_bound_mutating_lane(&fixture.plan, &mine, mutating, Some(&canonical))
            .unwrap_err();
    assert!(in_canonical.contains(&canonical), "{in_canonical}");

    // A directory that is not a checkout at all.
    let ordinary_folder = fixture.root.path().join("notes");
    std::fs::create_dir_all(&ordinary_folder).unwrap();
    assert!(refuse_unsafe_task_bound_mutating_lane(
        &fixture.plan,
        &mine,
        mutating,
        Some(ordinary_folder.to_str().unwrap()),
    )
    .is_err());

    // Read-only verification keeps its existing contract in the same checkout.
    refuse_unsafe_task_bound_mutating_lane(
        &fixture.plan,
        &mine,
        Some(&ExecutionIntent::ReadOnly),
        Some(&canonical),
    )
    .unwrap();

    // A non-repository task is untouched by the repository guard.
    let mut non_repo = mine.clone();
    non_repo.execution_intent = TaskExecutionIntent::NonRepository;
    refuse_unsafe_task_bound_mutating_lane(&fixture.plan, &non_repo, mutating, Some(&canonical))
        .unwrap();

    // A foreign task's lane is refused by owner, not by path shape.
    let theirs = fixture.theirs.clone();
    let foreign = fixture.lane_for(&theirs);
    let foreign_path = foreign.verify().unwrap().worktree_path;
    fixture.record_lane(&theirs, &foreign_path);
    let stolen =
        refuse_unsafe_task_bound_mutating_lane(&fixture.plan, &mine, mutating, Some(&foreign_path))
            .unwrap_err();
    assert!(stolen.contains(&theirs), "{stolen}");

    // Its own isolated lane is allowed: the guard refuses unsafe placement, not
    // mutating work.
    let own = fixture.lane_for(&fixture.mine);
    let own_path = own.verify().unwrap().worktree_path;
    refuse_unsafe_task_bound_mutating_lane(&fixture.plan, &mine, mutating, Some(&own_path))
        .unwrap();

    own.cleanup().unwrap();
    foreign.cleanup().unwrap();
}

#[test]
fn lane_provisioning_failure_is_a_synchronous_lane_dispatch_error() {
    let root = tempfile::tempdir().unwrap();
    let missing_checkout = root.path().join("missing-checkout");
    let provisioning_failure =
        provision_lane_for_test(missing_checkout.to_str().unwrap(), "plan-task-lane").unwrap_err();
    let error = super::super::dispatch_failed("lane_provision", provisioning_failure);

    assert_eq!(error.code, -32603);
    assert!(error.message.contains("dispatch failed at lane_provision"));
    assert_eq!(error.data.as_ref().unwrap()["type"], "task_dispatch_failed");
    assert_eq!(error.data.as_ref().unwrap()["stage"], "lane_provision");
    assert!(error.data.as_ref().unwrap()["reason"].is_string());
    assert!(error.data.as_ref().unwrap().get("tab_id").is_none());
}

fn canonical_of(lane: &Value) -> &str {
    lane.get("canonical_project_path")
        .and_then(Value::as_str)
        .expect("both payloads must name a canonical checkout")
}

fn worktree_of(lane: &Value) -> &str {
    lane.get("worktree_path")
        .and_then(Value::as_str)
        .expect("both payloads must name the worker worktree")
}

fn assert_payloads_agree(provenance: &WorkerLaneProvenance, project: &Path) {
    let canonical_project_path = project.display().to_string();
    let response = dispatch_response_lane(
        provenance,
        &canonical_project_path,
        &TaskLaneMetadata::default(),
    );
    assert_eq!(
        response["destination"].as_str(),
        provenance.destination.as_deref(),
        "the dispatch response reports the destination the lane persists"
    );
    let event = dispatch_ready_event(
        "pl_test",
        "task_test",
        "tab-worker",
        "attempt-1",
        &DispatchEvidence {
            session_id: "session-worker".into(),
            lane: Some(provenance.clone()),
        },
        &canonical_project_path,
        &[],
    );
    let emitted = event.get("lane").expect("dispatch_ready carries the lane");

    assert_eq!(canonical_of(&response), canonical_of(emitted));
    assert_eq!(worktree_of(&response), worktree_of(emitted));
    assert_eq!(canonical_of(&response), canonical_project_path);
    assert_ne!(
        canonical_of(&response),
        worktree_of(&response),
        "the worker lane must never be reported as the integration checkout"
    );
}

#[test]
fn response_and_dispatch_ready_share_canonical_project_path() {
    let fixture = fixture();

    // `verify_worker_lane` checks a pre-created lane against itself, so its
    // provenance carries the lane, not the project, in `canonical_cwd`.
    let pre_created = fixture.root.path().join("pre-created");
    super::git(
        &fixture.project,
        &[
            "worktree",
            "add",
            "-b",
            "lane/pre-created",
            pre_created.to_str().unwrap(),
        ],
    );
    let pre_created_provenance = resolve_dispatch_lane(
        Some(pre_created.to_str().unwrap()),
        Some(fixture.project.to_str().unwrap()),
        None,
        &TaskLaneMetadata::default(),
        LaneProtection::Enforced,
    )
    .unwrap()
    .existing
    .unwrap();
    assert_eq!(
        pre_created_provenance.canonical_cwd, pre_created_provenance.worktree_path,
        "the precondition: a lane verified against itself names itself canonical"
    );
    assert_payloads_agree(&pre_created_provenance, &fixture.project);

    // Provisioned lane: the same two payloads, the same agreement.
    let lane = fixture.lane_for(&fixture.mine);
    assert_payloads_agree(&lane.verify().unwrap(), &fixture.project);
    lane.cleanup().unwrap();

    super::git(
        &fixture.project,
        &[
            "worktree",
            "remove",
            "--force",
            pre_created.to_str().unwrap(),
        ],
    );
}
