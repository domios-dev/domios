use super::*;
use std::path::Path;

struct Fixture {
    root: tempfile::TempDir,
    lane: TeamCloseLane,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        // Git for Windows cannot create worktrees under a verbatim \\?\ path.
        let root_path = std::path::PathBuf::from(codeterm_paths::strip_verbatim_prefix(
            root.path().canonicalize().unwrap().to_str().unwrap(),
        ));
        let project = root_path.join("project");
        let path = root_path.join("lane");
        std::fs::create_dir(&project).unwrap();
        let cwd = project.to_str().unwrap();
        git(cwd, &["init", "-q", "--initial-branch=main"]).unwrap();
        git(cwd, &["config", "user.name", "Test"]).unwrap();
        git(cwd, &["config", "user.email", "test@example.com"]).unwrap();
        std::fs::write(project.join("proof"), "base").unwrap();
        git(cwd, &["add", "."]).unwrap();
        git(cwd, &["commit", "-qm", "base"]).unwrap();
        git(
            cwd,
            &[
                "worktree",
                "add",
                "-qb",
                "lane/clean",
                path.to_str().unwrap(),
            ],
        )
        .unwrap();
        Self {
            root,
            lane: TeamCloseLane {
                plan_id: "plan".into(),
                task_id: "task".into(),
                project_path: cwd.into(),
                worktree_path: Some(path.to_str().unwrap().into()),
                branch: Some("lane/clean".into()),
                base: Some("main".into()),
                destination: Some("HEAD".into()),
                hold_reason: None,
            },
        }
    }

    fn cwd(&self) -> &str {
        &self.lane.project_path
    }
    fn path(&self) -> &str {
        self.lane.worktree_path.as_deref().unwrap()
    }
    fn tip(&self) -> String {
        git(self.cwd(), &["rev-parse", "refs/heads/lane/clean"]).unwrap()
    }
    fn commit_lane(&self) {
        std::fs::write(Path::new(self.path()).join("proof"), "lane work").unwrap();
        git(self.path(), &["commit", "-qam", "lane work"]).unwrap();
    }
    fn remote(&self) -> String {
        let remote = self
            .root
            .path()
            .join("remote.git")
            .to_str()
            .unwrap()
            .to_string();
        git(self.cwd(), &["init", "--bare", &remote]).unwrap();
        git(self.cwd(), &["remote", "add", "origin", &remote]).unwrap();
        git(self.cwd(), &["push", "origin", "lane/clean"]).unwrap();
        remote
    }
    fn assert_kept(&self, tabs: &[LiveTabCwd]) {
        let tip = self.tip();
        let status = git(
            self.path(),
            &["status", "--porcelain", "--untracked-files=all"],
        )
        .unwrap();
        for confirm in [false, true] {
            let row = run_lane(&self.lane, confirm, tabs, None);
            assert_eq!(row.action, "kept", "{row:?}");
            assert!(!row.reason.is_empty());
            assert!(Path::new(self.path()).is_dir());
            assert_eq!(self.tip(), tip);
            assert_eq!(
                git(
                    self.path(),
                    &["status", "--porcelain", "--untracked-files=all"]
                )
                .unwrap(),
                status
            );
        }
    }
}

#[test]
fn lane_clean_selection_requires_terminal_or_review_and_no_live_worker() {
    for status in [
        TaskStatus::Pending,
        TaskStatus::InProgress,
        TaskStatus::Blocked,
        TaskStatus::Paused,
    ] {
        assert!(task_eligibility(&status, false, false).is_some());
    }
    for status in [
        TaskStatus::InReview,
        TaskStatus::Done,
        TaskStatus::Cancelled,
    ] {
        assert_eq!(task_eligibility(&status, false, false), None);
        assert!(task_eligibility(&status, true, false).is_some());
        assert!(task_eligibility(&status, false, true).is_some());
    }
}

#[test]
fn lane_clean_dry_run_is_offline_and_preserves_files_and_refs() {
    let fixture = Fixture::new();
    git(
        fixture.cwd(),
        &[
            "remote",
            "add",
            "origin",
            fixture.root.path().join("unreachable").to_str().unwrap(),
        ],
    )
    .unwrap();
    let tip = fixture.tip();
    let refs = git(fixture.cwd(), &["show-ref"]).unwrap();
    let row = run_lane(&fixture.lane, false, &[], None);
    assert_eq!(row.action, "remove");
    assert!(!row.reason.is_empty());
    assert!(Path::new(fixture.path()).is_dir());
    assert_eq!(fixture.tip(), tip);
    assert_eq!(git(fixture.cwd(), &["show-ref"]).unwrap(), refs);
    assert_eq!(
        std::fs::read(Path::new(fixture.path()).join("proof")).unwrap(),
        b"base"
    );
}

#[test]
fn lane_clean_confirm_removes_worktree_and_local_and_remote_branches() {
    let fixture = Fixture::new();
    let remote = fixture.remote();
    let row = run_lane(&fixture.lane, true, &[], None);
    assert_eq!(row.action, "removed", "{row:?}");
    assert!(!Path::new(fixture.path()).exists());
    assert!(git(
        fixture.cwd(),
        &["show-ref", "--verify", "refs/heads/lane/clean"]
    )
    .is_err());
    assert!(git(&remote, &["for-each-ref", "refs/heads/lane/clean"])
        .unwrap()
        .is_empty());
    assert!(!git(fixture.cwd(), &["worktree", "list", "--porcelain"])
        .unwrap()
        .contains(fixture.path()));
    assert_eq!(
        std::fs::read(Path::new(fixture.cwd()).join("proof")).unwrap(),
        b"base"
    );
}

#[test]
fn lane_clean_keeps_dirty_unmerged_and_occupied_lanes() {
    let dirty = Fixture::new();
    std::fs::write(Path::new(dirty.path()).join("untracked"), "preserve").unwrap();
    dirty.assert_kept(&[]);
    let unmerged = Fixture::new();
    unmerged.commit_lane();
    unmerged.assert_kept(&[]);
    let occupied = Fixture::new();
    std::fs::create_dir(Path::new(occupied.path()).join("subdirectory")).unwrap();
    occupied.assert_kept(&[LiveTabCwd {
        id: "shell".into(),
        title: "Shell".into(),
        cwd: format!("{}/subdirectory", occupied.path()),
    }]);
}

#[test]
fn lane_clean_keeps_lane_until_canonical_contains_the_recorded_destination() {
    let mut fixture = Fixture::new();
    fixture.commit_lane();
    git(fixture.cwd(), &["branch", "accepted", "lane/clean"]).unwrap();
    fixture.lane.destination = Some("accepted".into());
    let row = run_lane(&fixture.lane, true, &[], None);
    assert_eq!(row.action, "kept", "{row:?}");
    assert!(Path::new(fixture.path()).exists());
    assert!(git(
        fixture.cwd(),
        &["show-ref", "--verify", "refs/heads/lane/clean"]
    )
    .is_ok());
}

#[test]
fn lane_clean_checks_real_remote_tip_and_preserves_unmerged_remote_work() {
    let fixture = Fixture::new();
    let remote = fixture.remote();
    let cached = fixture.tip();
    fixture.commit_lane();
    let advanced = fixture.tip();
    git(
        &remote,
        &["fetch", fixture.cwd(), "lane/clean:refs/heads/lane/clean"],
    )
    .unwrap();
    git(
        fixture.cwd(),
        &["update-ref", "refs/remotes/origin/lane/clean", &cached],
    )
    .unwrap();
    git(fixture.path(), &["reset", "--hard", &cached]).unwrap();
    assert_eq!(run_lane(&fixture.lane, false, &[], None).action, "remove");
    assert_eq!(run_lane(&fixture.lane, true, &[], None).action, "kept");
    assert!(Path::new(fixture.path()).is_dir());
    assert_eq!(fixture.tip(), cached);
    assert_eq!(
        git(&remote, &["rev-parse", "refs/heads/lane/clean"]).unwrap(),
        advanced
    );
}

#[test]
fn lane_clean_remote_lease_refuses_a_tip_changed_after_assessment() {
    let fixture = Fixture::new();
    let remote = fixture.remote();
    let tips = remote_tips(&fixture.lane, true).unwrap();
    fixture.commit_lane();
    let advanced = fixture.tip();
    git(
        &remote,
        &["fetch", fixture.cwd(), "lane/clean:refs/heads/lane/clean"],
    )
    .unwrap();
    assert!(delete_remote_tips(&fixture.lane, &tips).is_err());
    assert_eq!(
        git(&remote, &["rev-parse", "refs/heads/lane/clean"]).unwrap(),
        advanced
    );
}

#[test]
fn lane_clean_refuses_detached_worktree_and_branch_checked_out_elsewhere() {
    let fixture = Fixture::new();
    git(fixture.path(), &["checkout", "--detach"]).unwrap();
    fixture.assert_kept(&[]);
    let mut branch_only = Fixture::new();
    branch_only.lane.worktree_path = None;
    assert_eq!(run_lane(&branch_only.lane, true, &[], None).action, "kept");
    assert!(git(
        branch_only.cwd(),
        &["show-ref", "--verify", "refs/heads/lane/clean"]
    )
    .is_ok());
}

#[test]
fn lane_clean_inventory_includes_attempts_and_branch_only_lanes_and_defaults_to_head() {
    let root = tempfile::tempdir().unwrap();
    let store = codeterm_service_core::service::plan::PlanStore::new_for_test(root.path());
    let mut plan = Plan::new(
        "plan".into(),
        root.path().display().to_string(),
        codeterm_service_core::plan::OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let id = store
        .add_task(
            &plan.id,
            codeterm_service_core::service::plan::NewTask {
                name: "task".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let mut plan = store.get(&plan.id).unwrap();
    let task = plan.tasks.iter_mut().find(|task| task.id == id).unwrap();
    task.status = TaskStatus::Done;
    task.lane.branch = Some("lane/current".into());
    task.lane.worktree_path = Some("current-path".into());
    task.lane.tab_id = Some("worker".into());
    task.execution_attempts
        .push(codeterm_service_core::plan::TaskExecutionAttempt {
            executor_tab_id: "former-worker".into(),
            branch: Some("lane/old".into()),
            worktree_path: Some("old-path".into()),
            destination: Some("accepted".into()),
            ..Default::default()
        });
    task.execution_attempts
        .push(codeterm_service_core::plan::TaskExecutionAttempt {
            executor_tab_id: "worker".into(),
            branch: Some("lane/current".into()),
            worktree_path: Some("current-path".into()),
            ..Default::default()
        });
    let lanes = cleanup_inventory(&plan);
    assert_eq!(lanes.len(), 2);
    assert_eq!(lanes[0].destination.as_deref(), Some("HEAD"));
    assert_eq!(lanes[1].destination.as_deref(), Some("accepted"));
    assert!(task_refusal(&plan.tasks[0], &lanes[0], &HashSet::from(["worker".into()])).is_some());
    assert!(task_refusal(
        &plan.tasks[0],
        &lanes[1],
        &HashSet::from(["former-worker".into()])
    )
    .is_some());
    assert!(task_refusal(&plan.tasks[0], &lanes[1], &HashSet::new()).is_none());
    plan.tasks[0].lane.worktree_path = None;
    assert!(
        cleanup_inventory(&plan)
            .iter()
            .any(|lane| lane.worktree_path.is_none()
                && lane.branch.as_deref() == Some("lane/current"))
    );
}

#[test]
fn lane_clean_rechecks_occupancy_after_remote_assessment_before_mutation() {
    let fixture = Fixture::new();
    let snapshots = std::cell::Cell::new(0);
    let row = run_lane_with(&fixture.lane, true, || {
        let count = snapshots.get() + 1;
        snapshots.set(count);
        let tabs = if count == 1 {
            vec![]
        } else {
            vec![LiveTabCwd {
                id: "arrived".into(),
                title: "Shell".into(),
                cwd: fixture.path().into(),
            }]
        };
        (tabs, None)
    });
    assert_eq!(snapshots.get(), 2);
    assert_eq!(row.action, "kept");
    assert!(Path::new(fixture.path()).exists());
    assert!(git(
        fixture.cwd(),
        &["show-ref", "--verify", "refs/heads/lane/clean"]
    )
    .is_ok());
}

#[cfg(unix)]
#[test]
fn lane_clean_removes_clean_tracked_symlinks_without_changing_their_targets() {
    let fixture = Fixture::new();
    let target = fixture.root.path().join("preserve-target");
    std::fs::write(&target, "preserve").unwrap();
    std::os::unix::fs::symlink(&target, Path::new(fixture.path()).join("tracked-link")).unwrap();
    git(fixture.path(), &["add", "tracked-link"]).unwrap();
    git(fixture.path(), &["commit", "-qm", "tracked link"]).unwrap();
    git(fixture.cwd(), &["merge", "--ff-only", "lane/clean"]).unwrap();
    let row = run_lane(&fixture.lane, true, &[], None);
    assert_eq!(row.action, "removed", "{row:?}");
    assert!(!Path::new(fixture.path()).exists());
    assert_eq!(std::fs::read(target).unwrap(), b"preserve");
}

#[test]
fn lane_clean_preview_labels_previously_removed_lane_absent() {
    let fixture = Fixture::new();
    let removed = run_lane(&fixture.lane, true, &[], None);
    assert_eq!(removed.action, "removed");
    for confirm in [false, true] {
        let row = run_lane(&fixture.lane, confirm, &[], None);
        assert_eq!(row.action, "absent", "{row:?}");
        assert!(row.reason.contains("already removed"));
    }
}

#[test]
fn lane_clean_preview_keeps_branch_only_cleanup_actionable() {
    let fixture = Fixture::new();
    git(fixture.cwd(), &["worktree", "remove", fixture.path()]).unwrap();
    let row = run_lane(&fixture.lane, false, &[], None);
    assert_eq!(row.action, "remove", "{row:?}");
}

#[test]
fn lane_clean_inventory_git_call_count_is_constant_for_historical_lanes() {
    for online in [false, true] {
        for lane_count in [1, 1000] {
            let mut calls = Vec::new();
            let inventory = Inventory::load_with("project", online, |_, args| {
                calls.push(args.join(" "));
                Ok(match args[0] {
                    "remote" => "origin\nbackup".into(),
                    "worktree" => "worktree /canonical\nbranch refs/heads/main".into(),
                    "for-each-ref" => "abc refs/heads/main".into(),
                    "rev-list" => "abc".into(),
                    "ls-remote" => "abc refs/heads/main".into(),
                    _ => panic!("unexpected inventory query: {args:?}"),
                })
            })
            .unwrap();
            for index in 0..lane_count {
                let lane = TeamCloseLane {
                    plan_id: "plan".into(),
                    task_id: index.to_string(),
                    project_path: "project".into(),
                    worktree_path: None,
                    branch: Some(format!("lane/absent-{index}")),
                    base: None,
                    destination: Some("HEAD".into()),
                    hold_reason: None,
                };
                let row = run_lane_with_inventory(&lane, online, &inventory, || (vec![], None));
                assert_eq!(row.action, "absent");
            }
            assert_eq!(
                calls
                    .iter()
                    .filter(|call| call.starts_with("worktree "))
                    .count(),
                1
            );
            assert_eq!(
                calls
                    .iter()
                    .filter(|call| call.starts_with("for-each-ref "))
                    .count(),
                1
            );
            assert_eq!(
                calls
                    .iter()
                    .filter(|call| call.starts_with("ls-remote "))
                    .count(),
                if online { 2 } else { 0 }
            );
            assert_eq!(calls.len(), if online { 6 } else { 4 });
        }
    }
}

#[test]
fn lane_clean_inventory_preserves_remote_only_and_registered_lanes() {
    let inventory = Inventory::load_with("project", true, |_, args| {
        Ok(match args[0] {
            "remote" => "origin".into(),
            "worktree" => "worktree /registered\nbranch refs/heads/lane/registered".into(),
            "ls-remote" => "abc refs/heads/lane/remote".into(),
            _ => String::new(),
        })
    })
    .unwrap();
    let mut lane = TeamCloseLane {
        plan_id: "plan".into(),
        task_id: "task".into(),
        project_path: "project".into(),
        worktree_path: None,
        branch: Some("lane/remote".into()),
        base: None,
        destination: Some("HEAD".into()),
        hold_reason: None,
    };
    assert!(!inventory.absent(&lane));
    lane.branch = None;
    lane.worktree_path = Some("/registered".into());
    assert!(!inventory.absent(&lane));
}

#[test]
#[ignore = "requires an explicit read-only plan and tab export"]
fn lane_clean_real_project_preview_measurement() {
    let input = std::path::PathBuf::from(std::env::var("CODETERM_LANE_PREVIEW_INPUT_DIR").unwrap());
    let plan: Plan =
        serde_json::from_slice(&std::fs::read(input.join("real-plan.json")).unwrap()).unwrap();
    let exported: serde_json::Value =
        serde_json::from_slice(&std::fs::read(input.join("real-tabs.json")).unwrap()).unwrap();
    let tabs: Vec<LiveTabCwd> = exported["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tab| LiveTabCwd {
            id: tab["id"].as_str().unwrap().into(),
            title: tab["title"].as_str().unwrap_or("Tab").into(),
            cwd: tab["cwd"].as_str().unwrap_or("").into(),
        })
        .collect();
    let live = tabs.iter().map(|tab| tab.id.clone()).collect();
    let started = std::time::Instant::now();
    let project = if plan.canonical_project_path.is_empty() {
        &plan.project_path
    } else {
        &plan.canonical_project_path
    };
    let lanes = cleanup_inventory(&plan);
    let inventory = Inventory::load(project, false).unwrap();
    let inventory_ms = started.elapsed().as_millis();
    let mut rows = Vec::new();
    for lane in &lanes {
        let remote = plan.tasks.iter().any(|task| {
            task.id == lane.task_id
                && task
                    .execution_attempts
                    .iter()
                    .any(|attempt| attempt.remote_worktree_path.is_some())
        });
        if inventory.absent(lane) && !remote {
            rows.push(LaneCleanRow {
                task_id: lane.task_id.clone(),
                worktree: lane.worktree_path.clone(),
                branch: lane.branch.clone(),
                action: "absent",
                reason: "lane worktree and branches already removed".into(),
            });
            continue;
        }
        let row = run_lane_with_inventory(lane, false, &inventory, || {
            let refusal = plan
                .tasks
                .iter()
                .filter(|task| {
                    task.id == lane.task_id
                        || task
                            .lane
                            .worktree_path
                            .as_ref()
                            .zip(lane.worktree_path.as_ref())
                            .is_some_and(|(a, b)| {
                                codeterm_service_core::service::worker_lane::same_path(a, b)
                            })
                        || task
                            .lane
                            .branch
                            .as_ref()
                            .zip(lane.branch.as_ref())
                            .is_some_and(|(a, b)| a == b)
                })
                .find_map(|task| task_refusal(task, lane, &live));
            (tabs.clone(), refusal)
        });
        rows.push(row);
    }
    let lanes_ms = started.elapsed().as_millis();
    for residue in codeterm_service_core::service::lane_removal::residue::inventory_with_worktrees(
        project,
        &tabs,
        &inventory.worktree_paths(),
    )
    .unwrap()
    {
        rows.push(LaneCleanRow {
            task_id: String::new(),
            worktree: Some(residue.path.display().to_string()),
            branch: None,
            action: if residue.refusal.is_some() {
                "kept"
            } else {
                "remove"
            },
            reason: residue
                .refusal
                .unwrap_or_else(|| "unregistered artifact residue".into()),
        });
    }
    let output = serde_json::json!({
        "project": project, "recorded_lanes": lanes.len(), "inventory_ms": inventory_ms,
        "lanes_ms": lanes_ms, "total_ms": started.elapsed().as_millis(), "lanes": rows,
    });
    std::fs::write(
        input.join("real-preview-result.json"),
        serde_json::to_vec_pretty(&output).unwrap(),
    )
    .unwrap();
    println!("project={project} recorded_lanes={} inventory_ms={inventory_ms} lanes_ms={lanes_ms} total_ms={}", lanes.len(), output["total_ms"]);
    assert!(rows
        .iter()
        .all(|row| matches!(row.action, "absent" | "kept" | "remove")));
}

#[test]
fn lane_clean_absence_never_overrides_remote_or_live_worker_refusal() {
    let lane = TeamCloseLane {
        plan_id: "plan".into(),
        task_id: "task".into(),
        project_path: "project".into(),
        worktree_path: None,
        branch: Some("lane/absent".into()),
        base: None,
        destination: Some("HEAD".into()),
        hold_reason: None,
    };
    for reason in [
        "remote lane requires cleanup on its owning peer",
        "lane Worker tab is live",
    ] {
        let row = run_lane_with_inventory(&lane, true, &Inventory::default(), || {
            (vec![], Some(reason.into()))
        });
        assert_eq!(row.action, "kept");
        assert_eq!(row.reason, reason);
    }
}

#[test]
fn lane_clean_batched_ancestry_distinguishes_merge_parents_and_checkout_heads() {
    let inventory = Inventory::load_with("/canonical", false, |_, args| {
        Ok(match args[0] {
            "worktree" => "worktree /canonical\nHEAD merged\nbranch refs/heads/main\n\nworktree /lane\nHEAD ahead\nbranch refs/heads/lane/work".into(),
            "for-each-ref" => "merged refs/heads/main\nahead refs/heads/lane/work\nleft refs/heads/lane/merged".into(),
            "rev-list" => "merged left right\nleft base\nright base\nahead left\nbase".into(),
            "remote" => String::new(),
            _ => panic!("unexpected query"),
        })
    }).unwrap();
    assert_eq!(
        inventory.merged_refusal("/canonical", "refs/heads/lane/merged", "HEAD"),
        None
    );
    assert!(inventory
        .merged_refusal("/canonical", "refs/heads/lane/work", "HEAD")
        .is_some());
    assert_eq!(inventory.merged_refusal("/lane", "HEAD", "lane/work"), None);
    assert!(inventory.merged_refusal("/lane", "HEAD", "main").is_some());
    assert_eq!(
        inventory.merged_refusal("/canonical", "right", "main"),
        None
    );
    assert_eq!(inventory.merged_refusal("/canonical", "base", "main"), None);
    assert_eq!(
        inventory
            .merged_refusal("/canonical", "base", "-invalid")
            .as_deref(),
        Some("invalid lane ancestry reference")
    );
}
