//! Team close and its lane sinks against real repositories: a checkout the
//! caller bound is never removed, while a lane Domios provisioned still is.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::super::close::{cleanup_lane_with_tabs, finish_team_lane_cleanup, TeamCloseLane};
use codeterm_service_core::plan::{LaneOwnership, OrchestratorRef, Plan};
use codeterm_service_core::service::plan::lane_ownership::FOREIGN_CHECKOUT;
use codeterm_service_core::service::plan::{NewTask, PlanStore};

fn git(dir: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    codeterm_service_core::process_util::no_window_std(&mut command);
    let output = command.current_dir(dir).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn repository(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q", "--initial-branch=main"]);
    git(path, &["config", "user.email", "test@example.com"]);
    git(path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("README.md"), "user content").unwrap();
    git(path, &["add", "README.md"]);
    git(path, &["commit", "-qm", "user commit"]);
    git(path, &["branch", "develop"]);
}

fn refs(repository: &Path) -> String {
    git(
        repository,
        &["for-each-ref", "--format=%(objectname) %(refname)"],
    )
}

struct Scene {
    _root: tempfile::TempDir,
    store: PlanStore,
    plan_id: String,
    domios_task: String,
    foreign_task: String,
    canonical: PathBuf,
    domios_lane: PathBuf,
    foreign: PathBuf,
    foreign_linked: PathBuf,
}

impl Scene {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        // Git for Windows cannot create worktrees under a verbatim \\?\ path.
        let base = PathBuf::from(codeterm_paths::strip_verbatim_prefix(
            root.path().canonicalize().unwrap().to_str().unwrap(),
        ));
        let canonical = base.join("project");
        repository(&canonical);
        let domios_lane = base.join("project-worktrees").join("task-a1b2");
        git(
            &canonical,
            &[
                "worktree",
                "add",
                "-qb",
                "lane/task-a1b2",
                domios_lane.to_str().unwrap(),
            ],
        );
        let foreign = base.join("q6fc-foreign");
        repository(&foreign);
        let foreign_linked = base.join("q6fc-foreign-feature");
        git(
            &foreign,
            &[
                "worktree",
                "add",
                "-qb",
                "feature",
                foreign_linked.to_str().unwrap(),
            ],
        );
        let store = PlanStore::new_for_test(&base);
        let mut plan = Plan::new(
            "foreign checkout".into(),
            canonical.display().to_string(),
            OrchestratorRef::from_tab("owner"),
        );
        store.save(&mut plan, 1).unwrap();
        let task = |name: &str| {
            store
                .add_task(
                    &plan.id,
                    NewTask {
                        name: name.into(),
                        ..Default::default()
                    },
                    2,
                )
                .unwrap()
        };
        let domios_task = task("domios");
        let foreign_task = task("foreign");
        Self {
            _root: root,
            plan_id: plan.id.clone(),
            store,
            domios_task,
            foreign_task,
            canonical,
            domios_lane,
            foreign,
            foreign_linked,
        }
    }

    fn lane(
        &self,
        task: &str,
        worktree: Option<&Path>,
        branch: &str,
        ownership: Option<LaneOwnership>,
    ) -> TeamCloseLane {
        TeamCloseLane {
            plan_id: self.plan_id.clone(),
            task_id: task.into(),
            project_path: self.canonical.display().to_string(),
            worktree_path: worktree.map(|path| path.display().to_string()),
            branch: Some(branch.into()),
            base: None,
            destination: Some("HEAD".into()),
            hold_reason: None,
            ownership,
        }
    }

    /// Every shape a caller-bound checkout takes: the repository root with and
    /// without a record, the user's own linked worktree, and a bare branch
    /// whose name also exists, merged and not checked out, in the canonical repository.
    fn foreign_lanes(&self) -> Vec<TeamCloseLane> {
        let foreign = Some(LaneOwnership::ForeignCheckout);
        vec![
            self.lane(&self.foreign_task, Some(&self.foreign), "main", foreign),
            self.lane(&self.foreign_task, Some(&self.foreign), "main", None),
            self.lane(
                &self.foreign_task,
                Some(&self.foreign_linked),
                "feature",
                foreign,
            ),
            self.lane(&self.foreign_task, None, "develop", foreign),
            self.lane(&self.foreign_task, None, "develop", None),
        ]
    }
}

#[test]
fn team_close_never_removes_a_foreign_checkout_and_still_cleans_domios_lanes() {
    let scene = Scene::new();
    let canonical_refs = refs(&scene.canonical);
    let foreign_refs = refs(&scene.foreign);
    let foreign_worktrees = git(&scene.foreign, &["worktree", "list", "--porcelain"]);
    let mut lanes = vec![scene.lane(
        &scene.domios_task,
        Some(&scene.domios_lane),
        "lane/task-a1b2",
        Some(LaneOwnership::DomiosLane),
    )];
    lanes.extend(scene.foreign_lanes());

    let inventory = finish_team_lane_cleanup(&scene.store, &lanes, 10).unwrap();

    assert_eq!(inventory.removed.len(), 1, "{inventory:?}");
    assert_eq!(
        inventory.removed[0].branch.as_deref(),
        Some("lane/task-a1b2")
    );
    assert!(inventory.kept.is_empty(), "{inventory:?}");
    assert_eq!(inventory.not_managed.len(), 5, "{inventory:?}");
    for (record, lane) in inventory.not_managed.iter().zip(scene.foreign_lanes()) {
        let reason = record.reason.as_deref().unwrap();
        assert!(reason.contains("not managed"), "{reason}");
        if lane.ownership == Some(LaneOwnership::ForeignCheckout) {
            assert_eq!(reason, FOREIGN_CHECKOUT);
        }
    }
    assert!(!scene.domios_lane.exists());
    assert!(!refs(&scene.canonical).contains("refs/heads/lane/task-a1b2"));
    assert_eq!(
        refs(&scene.canonical),
        canonical_refs
            .lines()
            .filter(|line| !line.ends_with("refs/heads/lane/task-a1b2"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert_eq!(refs(&scene.foreign), foreign_refs);
    assert_eq!(
        git(&scene.foreign, &["worktree", "list", "--porcelain"]),
        foreign_worktrees
    );
    assert_eq!(
        std::fs::read_to_string(scene.foreign.join("README.md")).unwrap(),
        "user content"
    );
    assert!(scene.foreign_linked.join("README.md").is_file());

    let foreign_task = scene
        .store
        .inspect_task_for_test(&scene.foreign_task)
        .unwrap()
        .task;
    assert_eq!(foreign_task.lane.cleanup_status.as_deref(), Some("done"));
    assert!(foreign_task
        .lane
        .cleanup_hints
        .iter()
        .any(|hint| hint.contains(FOREIGN_CHECKOUT)));
}

#[test]
fn lane_sink_refuses_every_foreign_shape_before_touching_git() {
    let scene = Scene::new();
    let canonical_refs = refs(&scene.canonical);
    let foreign_refs = refs(&scene.foreign);
    for lane in scene.foreign_lanes() {
        let refusal = cleanup_lane_with_tabs(&lane, &[]).unwrap_err();
        assert!(refusal.contains("not managed"), "{lane:?}: {refusal}");
    }
    assert_eq!(refs(&scene.canonical), canonical_refs);
    assert_eq!(refs(&scene.foreign), foreign_refs);
    assert!(scene.foreign_linked.is_dir());
}
