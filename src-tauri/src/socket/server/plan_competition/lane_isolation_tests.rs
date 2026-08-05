use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use crate::plan::{ExecutionIntent, OrchestratorRef, Plan};
use crate::service::plan::task_attempts::TaskCompeteDraft;
use crate::service::plan::{NewTask, PlanStore};

use super::{mutating_competition, provision_competitor_lane};

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
    assert_eq!(worktrees.len(), 3, "N competitors need N distinct worktrees");
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
