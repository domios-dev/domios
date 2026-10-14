use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use crate::plan::{ExecutionIntent, OrchestratorRef, Plan};
use crate::service::plan::task_attempts::TaskCompeteDraft;
use crate::service::plan::{NewTask, PlanStore};

use super::{
    compensate_current_competitor, compensate_spawned_competitors, mutating_competition,
    provision_all_competitor_lanes, provision_competitor_lane, CompetitorCompensation,
};

fn git(dir: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    crate::process_util::no_window_std(&mut command);
    let output = command.current_dir(dir).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repo_plan(intent: ExecutionIntent) -> (tempfile::TempDir, PlanStore, Plan, String) {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q"]);
    git(&project, &["config", "user.email", "test@example.com"]);
    git(&project, &["config", "user.name", "Test"]);
    std::fs::write(project.join("README.md"), "fixture").unwrap();
    git(&project, &["add", "README.md"]);
    git(&project, &["commit", "-qm", "fixture"]);

    let store = PlanStore::new_for_test(root.path());
    let created = store
        .create_plan_for_team(
            "Competition".into(),
            project.display().to_string(),
            OrchestratorRef::from_tab("orch"),
            "team".into(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &created.id,
            NewTask {
                name: "compete".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let mut plan = store.get(&created.id).unwrap();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.interruption.execution_intent = intent;
    task.lane.destination = Some("develop".into());
    (root, store, plan, task_id)
}

fn draft(plan: &Plan, task_id: &str, lane: u32) -> TaskCompeteDraft {
    TaskCompeteDraft {
        plan_id: plan.id.clone(),
        task_id: task_id.to_string(),
        competition_group_id: "compete_x".into(),
        lane,
        provider: "claude".into(),
        planned_model: None,
        model: None,
        reasoning_effort: None,
        parent_tab_id: Some("orch".into()),
        workspace_id: None,
        title: None,
        cwd: plan.canonical_project_path.clone(),
        branch: None,
        base: None,
        destination: Some("develop".into()),
        task_prompt: "brief".into(),
        report_command: "codeterm agent report".into(),
        plan_skill_manifest: vec![],
    }
}

#[test]
fn mutating_competition_gives_each_attempt_its_own_lane() {
    let (_root, _store, plan, task_id) = repo_plan(ExecutionIntent::Mutating);
    assert!(mutating_competition(&plan, &task_id));

    let mut drafts: Vec<TaskCompeteDraft> =
        (1..=3).map(|lane| draft(&plan, &task_id, lane)).collect();
    let lanes: Vec<_> = drafts
        .iter_mut()
        .map(|draft| provision_competitor_lane(&plan, &task_id, draft).unwrap())
        .collect();

    let worktrees: HashSet<String> = drafts
        .iter()
        .map(|draft| codeterm_paths::normalize_cwd(&draft.cwd))
        .collect();
    let branches: HashSet<String> = drafts
        .iter()
        .map(|draft| draft.branch.clone().unwrap())
        .collect();
    assert_eq!(
        worktrees.len(),
        3,
        "N competitors need N distinct worktrees"
    );
    assert_eq!(branches.len(), 3, "N competitors need N distinct branches");
    for draft in &drafts {
        assert!(Path::new(&draft.cwd).is_dir());
        assert!(draft.base.is_some());
        assert_eq!(draft.destination.as_deref(), Some("develop"));
    }
    for lane in lanes {
        lane.cleanup().unwrap();
    }
}

#[test]
fn mutating_competition_never_uses_canonical_or_a_shared_lane() {
    let (_root, _store, mut plan, task_id) = repo_plan(ExecutionIntent::Mutating);
    let canonical = codeterm_paths::normalize_cwd(&plan.canonical_project_path);
    // A recorded shared lane must not be adopted either.
    plan.tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap()
        .lane
        .worktree_path = Some(plan.canonical_project_path.clone());

    let mut first = draft(&plan, &task_id, 1);
    let mut second = draft(&plan, &task_id, 2);
    let lane_a = provision_competitor_lane(&plan, &task_id, &mut first).unwrap();
    let lane_b = provision_competitor_lane(&plan, &task_id, &mut second).unwrap();

    for draft in [&first, &second] {
        let resolved = codeterm_paths::normalize_cwd(&draft.cwd);
        assert_ne!(resolved, canonical);
        assert!(draft.branch.as_deref().is_some_and(|b| b != "develop"));
    }
    assert_ne!(
        codeterm_paths::normalize_cwd(&first.cwd),
        codeterm_paths::normalize_cwd(&second.cwd)
    );
    lane_a.cleanup().unwrap();
    lane_b.cleanup().unwrap();
}

#[test]
fn read_only_competition_is_unchanged() {
    for intent in [ExecutionIntent::ReadOnly, ExecutionIntent::Unspecified] {
        let (_root, _store, plan, task_id) = repo_plan(intent);
        // Negative control: no provisioning is even attempted, so the draft keeps
        // the shared cwd it has today and records no lane provenance.
        assert!(!mutating_competition(&plan, &task_id));
        let draft = draft(&plan, &task_id, 1);
        assert_eq!(draft.cwd, plan.canonical_project_path);
        assert!(draft.branch.is_none());
        assert!(draft.base.is_none());
    }
}

fn competitor(index: u32, worktree: Option<&str>) -> CompetitorCompensation {
    CompetitorCompensation {
        tab_id: format!("tab-c{index}"),
        attempt_id: format!("attempt-c{index}"),
        worktree_path: worktree.map(str::to_string),
    }
}

#[test]
fn provisioning_failure_before_spawn_leaves_zero_tabs_attempts_and_worktrees() {
    // A non-git canonical path makes `git worktree add` fail on the FIRST
    // competitor, so nothing is spawned and nothing is left behind.
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let store = PlanStore::new_for_test(root.path());
    let created = store
        .create_plan_for_team(
            "Competition".into(),
            project.display().to_string(),
            OrchestratorRef::from_tab("orch"),
            "team".into(),
            vec![],
            1,
        )
        .unwrap();
    let task_id = store
        .add_task(
            &created.id,
            NewTask {
                name: "compete".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let mut plan = store.get(&created.id).unwrap();
    plan.tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap()
        .interruption
        .execution_intent = ExecutionIntent::Mutating;
    let drafts: Vec<TaskCompeteDraft> = (1..=2).map(|lane| draft(&plan, &task_id, lane)).collect();

    let error = provision_all_competitor_lanes(&plan, &task_id, drafts).unwrap_err();

    assert_eq!(error.data.unwrap()["stage"], "lane_provision");
    let container = root.path().join("project-worktrees");
    assert!(
        !container.is_dir() || std::fs::read_dir(&container).unwrap().next().is_none(),
        "a failed provisioning round must leave no worktree"
    );
}

#[test]
fn spawn_failure_leaves_no_live_tab_with_a_deleted_worktree() {
    let order = std::cell::RefCell::new(Vec::<String>::new());
    let outcome = compensate_spawned_competitors(
        vec![
            competitor(1, Some("/lanes/c1")),
            competitor(2, Some("/lanes/c2")),
        ],
        |tab| {
            order.borrow_mut().push(format!("close:{tab}"));
            Ok(())
        },
        |tab| {
            order.borrow_mut().push(format!("retire:{tab}"));
            Ok(())
        },
        |path| {
            order.borrow_mut().push(format!("cleanup:{path}"));
            Ok(())
        },
    );
    let order = order.into_inner();

    assert_eq!(outcome.released.len(), 2);
    assert!(outcome.retained.is_empty());
    // Ownership order per competitor: stop, retire, THEN delete the lane.
    assert_eq!(
        order,
        vec![
            "close:tab-c1",
            "retire:tab-c1",
            "cleanup:/lanes/c1",
            "close:tab-c2",
            "retire:tab-c2",
            "cleanup:/lanes/c2",
        ]
    );
}

#[test]
fn a_tab_that_cannot_close_keeps_its_worktree_and_is_reported() {
    let cleaned = std::cell::RefCell::new(Vec::<String>::new());
    let outcome = compensate_spawned_competitors(
        vec![competitor(1, Some("/lanes/c1"))],
        |_| Err("tab is wedged".into()),
        |_| panic!("retire must not run once the tab could not be stopped"),
        |path| {
            cleaned.borrow_mut().push(path.to_string());
            Ok(())
        },
    );
    let cleaned = cleaned.into_inner();

    assert!(outcome.released.is_empty());
    assert!(cleaned.is_empty(), "never delete under a live tab");
    assert_eq!(outcome.retained[0]["worktree_path"], "/lanes/c1");
    assert_eq!(outcome.retained[0]["reason"], "tab is wedged");
    assert_eq!(outcome.retained[0]["attempt_id"], "attempt-c1");
}

#[test]
fn an_attempt_that_cannot_retire_keeps_its_worktree() {
    let cleaned = std::cell::RefCell::new(Vec::<String>::new());
    let outcome = compensate_spawned_competitors(
        vec![competitor(1, Some("/lanes/c1"))],
        |_| Ok(()),
        |_| Err("attempt is pending a write".into()),
        |path| {
            cleaned.borrow_mut().push(path.to_string());
            Ok(())
        },
    );
    let cleaned = cleaned.into_inner();

    assert!(cleaned.is_empty(), "never delete under a live attempt");
    assert_eq!(outcome.retained.len(), 1);
    assert_eq!(outcome.retained[0]["reason"], "attempt is pending a write");
}

#[test]
fn a_failed_lane_cleanup_is_retained_rather_than_reported_released() {
    let outcome = compensate_spawned_competitors(
        vec![competitor(1, Some("/lanes/c1"))],
        |_| Ok(()),
        |_| Ok(()),
        |_| Err("worktree is locked".into()),
    );

    assert!(outcome.released.is_empty());
    assert_eq!(outcome.retained[0]["reason"], "worktree is locked");
}

#[test]
fn successful_competitors_retain_their_lanes() {
    let (_root, _store, plan, task_id) = repo_plan(ExecutionIntent::Mutating);
    let drafts: Vec<TaskCompeteDraft> = (1..=3).map(|lane| draft(&plan, &task_id, lane)).collect();

    let (drafts, lanes) = provision_all_competitor_lanes(&plan, &task_id, drafts).unwrap();

    assert_eq!(lanes.iter().filter(|lane| lane.is_some()).count(), 3);
    for draft in &drafts {
        assert!(Path::new(&draft.cwd).is_dir(), "no compensation ran");
    }
    for lane in lanes.into_iter().flatten() {
        lane.cleanup().unwrap();
    }
}

/// Record failed AND the new tab could not be closed: it is still live, so its
/// worktree must survive and be reported.
#[test]
fn record_failure_with_a_stuck_tab_retains_the_current_lane() {
    let cleaned = std::cell::RefCell::new(Vec::<String>::new());
    let outcome = compensate_current_competitor(
        "tab-current",
        Some("/lanes/current"),
        Some("close_tab failed".into()),
        |path| {
            cleaned.borrow_mut().push(path.to_string());
            Ok(())
        },
    );
    let cleaned = cleaned.into_inner();

    assert!(cleaned.is_empty(), "never delete under a still-live tab");
    assert!(outcome.released.is_empty());
    assert_eq!(outcome.retained.len(), 1);
    assert_eq!(outcome.retained[0]["tab_id"], "tab-current");
    assert_eq!(outcome.retained[0]["worktree_path"], "/lanes/current");
    assert_eq!(outcome.retained[0]["reason"], "close_tab failed");
    // No attempt was recorded, so none is claimed as retired.
    assert!(outcome.retained[0]["attempt_id"].is_null());
}

/// Record failed but the tab closed cleanly: the lane is unowned, so it is
/// deleted — and retirement is still never attempted for an unrecorded attempt.
#[test]
fn record_failure_with_a_closed_tab_releases_the_current_lane() {
    let cleaned = std::cell::RefCell::new(Vec::<String>::new());
    let outcome =
        compensate_current_competitor("tab-current", Some("/lanes/current"), None, |path| {
            cleaned.borrow_mut().push(path.to_string());
            Ok(())
        });
    let cleaned = cleaned.into_inner();

    assert_eq!(cleaned, vec!["/lanes/current"]);
    assert!(outcome.retained.is_empty());
    assert_eq!(outcome.released.len(), 1);
    assert_eq!(outcome.released[0]["worktree_path"], "/lanes/current");
    assert!(outcome.released[0]["attempt_id"].is_null());
}

#[test]
fn a_read_only_current_competitor_has_no_lane_to_release() {
    let outcome = compensate_current_competitor("tab-current", None, None, |_| {
        panic!("there is no lane to clean")
    });
    assert_eq!(outcome.released.len(), 1);
    assert!(outcome.released[0]["worktree_path"].is_null());
}
