use super::*;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn git_output(cwd: &Path, args: &[&str]) -> Output {
    let mut command = Command::new("git");
    crate::process_util::no_window_std(&mut command);
    command.current_dir(cwd).args(args).output().unwrap()
}

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = git_output(cwd, args);
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn repository(root: &Path, name: &str) -> PathBuf {
    let cwd = root.join(name);
    std::fs::create_dir(&cwd).unwrap();
    git(&cwd, &["init", "-q"]);
    git(&cwd, &["config", "user.email", "fixture@example.com"]);
    git(&cwd, &["config", "user.name", "Fixture"]);
    git(&cwd, &["config", "commit.gpgsign", "false"]);
    git(&cwd, &["symbolic-ref", "HEAD", "refs/heads/develop"]);
    std::fs::write(cwd.join("owned.txt"), name).unwrap();
    git(&cwd, &["add", "owned.txt"]);
    git(&cwd, &["commit", "-qm", name]);
    cwd.canonicalize().unwrap()
}

#[test]
fn respawn_cross_repository_lane_resolves_own_base_and_persists_replacement() {
    let root = tempfile::tempdir().unwrap();
    let repo_a = repository(root.path(), "plan-repo");
    let repo_b = repository(root.path(), "extracted-repo");
    let base = git(&repo_b, &["rev-parse", "HEAD"]);
    let branch = "lane/sandbox-rollout";
    let lane = root.path().join("extracted-repo-worktrees");
    git(
        &repo_b,
        &[
            "worktree",
            "add",
            "-qb",
            branch,
            lane.to_str().unwrap(),
            &base,
        ],
    );
    let lane_path = lane.to_str().unwrap();
    assert!(git(&lane, &["status", "--porcelain"]).is_empty());
    assert_eq!(git(&lane, &["symbolic-ref", "--short", "HEAD"]), branch);
    assert_eq!(git(&lane, &["rev-parse", "HEAD"]), base);
    let old_lookup = git_output(
        &repo_a,
        &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
    );
    assert_eq!(old_lookup.status.code(), Some(128));
    assert!(String::from_utf8_lossy(&old_lookup.stderr).contains("Needed a single revision"));
    let store = PlanStore::new_for_test(root.path());
    let (plan_id, task, source_draft) = respawn_fixture(
        &store,
        repo_a.to_str().unwrap(),
        TaskExecutionIntent::RepositoryMutation,
        ExecutionIntent::Mutating,
    );
    store
        .with_plan(&plan_id, 6, |plan| {
            let task = &mut plan.tasks[0];
            task.lane.worktree_path = Some(lane_path.into());
            task.lane.branch = Some(branch.into());
            task.lane.base = Some(base.clone());
            task.lane.destination = Some("develop".into());
            let source = &mut task.execution_attempts[0];
            source.worktree_path = task.lane.worktree_path.clone();
            source.branch = task.lane.branch.clone();
            source.base = task.lane.base.clone();
            Ok(())
        })
        .unwrap();
    let draft = store
        .build_task_respawn_draft(
            &plan_id,
            &task.id,
            TaskRespawnRequest {
                attempt_id: source_draft.source_attempt.id.clone(),
                ..Default::default()
            },
        )
        .unwrap();
    let task = store.show_task(&task.id).unwrap().task;
    let before = store.get(&plan_id).unwrap();
    let provenance = verify_respawn_lane(repo_a.to_str().unwrap(), &task, &draft)
        .unwrap()
        .unwrap();
    assert_eq!(provenance.base, base);
    assert_eq!(provenance.branch, branch);
    assert_eq!(provenance.destination.as_deref(), Some("develop"));
    assert!(crate::service::worker_lane::same_path(
        &provenance.canonical_cwd,
        repo_b.to_str().unwrap()
    ));
    assert_eq!(store.get(&plan_id).unwrap(), before);

    let mut missing_base = task.clone();
    missing_base.lane.base = Some("refs/heads/missing-base".into());
    let error = verify_respawn_lane(repo_a.to_str().unwrap(), &missing_base, &draft).unwrap_err();
    assert!(error.contains("missing-base"));
    assert!(error.contains("owning repository"));
    let unrelated = git(
        &repo_b,
        &["commit-tree", "HEAD^{tree}", "-m", "unrelated-base"],
    );
    let mut nonancestor = task.clone();
    nonancestor.lane.base = Some(unrelated);
    assert!(
        verify_respawn_lane(repo_a.to_str().unwrap(), &nonancestor, &draft)
            .unwrap_err()
            .contains("not an ancestor")
    );
    let mut foreign_base = task.clone();
    foreign_base.lane.base = Some(git(&repo_a, &["rev-parse", "HEAD"]));
    assert!(
        verify_respawn_lane(repo_a.to_str().unwrap(), &foreign_base, &draft)
            .unwrap_err()
            .contains("does not resolve in owning repository")
    );
    git(&lane, &["checkout", "-q", "--detach"]);
    assert!(verify_respawn_lane(repo_a.to_str().unwrap(), &task, &draft)
        .unwrap_err()
        .contains("detached HEAD"));
    git(&lane, &["checkout", "-q", branch]);
    let mut wrong_branch = task.clone();
    wrong_branch.lane.branch = Some("lane/foreign".into());
    assert!(
        verify_respawn_lane(repo_a.to_str().unwrap(), &wrong_branch, &draft)
            .unwrap_err()
            .contains("branch mismatch")
    );
    let mut primary_task = task.clone();
    primary_task.lane.branch = Some("develop".into());
    let mut primary_draft = draft.clone();
    primary_draft.cwd = repo_b.display().to_string();
    assert!(
        verify_respawn_lane(repo_a.to_str().unwrap(), &primary_task, &primary_draft)
            .unwrap_err()
            .contains("primary checkout")
    );

    let created = store
        .record_task_respawn_attempt(&plan_id, &task.id, "replacement", &draft, Some("worker"), 7)
        .unwrap();
    store
        .finish_task_dispatch(
            &plan_id,
            &task.id,
            "replacement",
            &created.attempt.id,
            "replacement-session",
            "replacement-nonce",
            task.lane.clone(),
            8,
        )
        .unwrap();
    store
        .acknowledge_live_task_attempt_for_tab("replacement", None, None, 9)
        .unwrap();
    let restored = store.show_task(&task.id).unwrap().task;
    assert_eq!(restored.status, TaskStatus::InProgress);
    assert_eq!(restored.lane.worktree_path, task.lane.worktree_path);
    assert_eq!(restored.lane.base, Some(base));
    assert_eq!(restored.lane.branch.as_deref(), Some(branch));
    assert!(restored
        .execution_attempts
        .iter()
        .find(|attempt| attempt.id == draft.source_attempt.id)
        .unwrap()
        .closed_at
        .is_some());
    assert_eq!(
        restored
            .execution_attempts
            .iter()
            .find(|attempt| attempt.id == created.attempt.id)
            .unwrap()
            .acknowledged_at,
        Some(9)
    );
}
