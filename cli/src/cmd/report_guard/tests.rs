use super::*;
use crate::cmd::report_attribution::ReportBase;
use std::fs;
use tempfile::TempDir;

fn run(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn repo() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    run(dir.path(), &["init", "--initial-branch", "main"]);
    run(
        dir.path(),
        &["config", "user.email", "test@example.invalid"],
    );
    run(dir.path(), &["config", "user.name", "Test User"]);
    fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    run(dir.path(), &["add", "a.txt"]);
    run(dir.path(), &["commit", "-m", "initial"]);
    dir
}

fn repo_with_linked_worktree() -> (TempDir, PathBuf, PathBuf) {
    let dir = repo();
    let main = dir.path().to_path_buf();
    let linked = dir.path().join("linked");
    let linked_str = linked.to_string_lossy().into_owned();
    run(
        dir.path(),
        &["worktree", "add", "-b", "lane/report", &linked_str],
    );
    (dir, main, linked)
}

fn branch_head(dir: &Path) -> String {
    git_one(dir, &["rev-parse", "HEAD"]).unwrap()
}

#[test]
fn dirty_readiness_report_is_refused() {
    let dir = repo();
    fs::write(dir.path().join("a.txt"), "dirty\n").unwrap();

    let err = enrich_ready_report_in(
        dir.path(),
        Some(ReportStatusArg::InReview),
        None,
        None,
        None,
        None,
        false,
    )
    .unwrap_err();

    assert!(err.message().contains("readiness report refused"));
    assert!(err.message().contains("--status partial"));
    assert_eq!(err.dirty_files, vec!["a.txt"]);
}

#[test]
fn acknowledge_dirty_skips_dirty_check() {
    let dir = repo();
    fs::write(dir.path().join("a.txt"), "dirty\n").unwrap();

    let (branch, sha, changed) = enrich_ready_report_in(
        dir.path(),
        Some(ReportStatusArg::InReview),
        None,
        None,
        None,
        None,
        true,
    )
    .unwrap();

    assert!(sha.unwrap().len() >= 7);
    assert!(changed.unwrap().contains(&"a.txt".to_string()));
    assert_eq!(branch.as_deref(), Some("main"));
}

#[test]
fn tool_owned_plan_mirror_dirt_is_ignored() {
    let dir = repo();
    let plans_dir = dir.path().join(".codeterm/plans");
    fs::create_dir_all(&plans_dir).unwrap();
    fs::write(plans_dir.join("mirror.md"), "tool state\n").unwrap();

    let (_branch, sha, _changed) = enrich_ready_report_in(
        dir.path(),
        Some(ReportStatusArg::InReview),
        None,
        None,
        None,
        None,
        false,
    )
    .unwrap();

    assert!(sha.unwrap().len() >= 7);
}

#[test]
fn mixed_tool_and_lane_dirt_refuses_only_lane_files() {
    let dir = repo();
    let plans_dir = dir.path().join(".codeterm/plans");
    fs::create_dir_all(&plans_dir).unwrap();
    fs::write(plans_dir.join("mirror.md"), "tool state\n").unwrap();
    fs::write(dir.path().join("src.rs"), "lane dirt\n").unwrap();

    let err = enrich_ready_report_in(
        dir.path(),
        Some(ReportStatusArg::InReview),
        None,
        None,
        None,
        None,
        false,
    )
    .unwrap_err();

    assert_eq!(err.dirty_files, vec!["src.rs"]);
    assert!(!err.message().contains(".codeterm/plans"));
}

#[test]
fn clean_repo_returns_head_metadata() {
    let dir = repo();

    let metadata = inspect_git_worktree(dir.path(), Some("HEAD"))
        .unwrap()
        .unwrap();

    assert!(metadata.sha.unwrap().len() >= 7);
    assert!(metadata.changed_files.unwrap().is_empty());
}

#[test]
fn modified_file_reports_exactly_one_changed_path() {
    let dir = repo();
    fs::write(dir.path().join("a.txt"), "dirty\n").unwrap();

    let metadata = inspect_git_worktree(dir.path(), Some("HEAD"))
        .unwrap()
        .unwrap();

    assert_eq!(metadata.changed_files.unwrap(), vec!["a.txt"]);
}

#[test]
fn untracked_file_reports_exactly_one_changed_path() {
    let dir = repo();
    fs::write(dir.path().join("new.txt"), "new\n").unwrap();

    let metadata = inspect_git_worktree(dir.path(), Some("HEAD"))
        .unwrap()
        .unwrap();

    assert_eq!(metadata.changed_files.unwrap(), vec!["new.txt"]);
}

#[test]
fn linked_worktree_stays_clean_when_main_checkout_is_dirty() {
    let (_dir, main, linked) = repo_with_linked_worktree();
    fs::write(main.join("a.txt"), "dirty main\n").unwrap();

    let metadata = inspect_git_worktree(&linked, Some("HEAD"))
        .unwrap()
        .unwrap();

    assert!(metadata.changed_files.unwrap().is_empty());
}

#[test]
fn one_committed_lane_commit_reports_its_files_since_base() {
    let dir = repo();
    let base = branch_head(dir.path());
    fs::write(dir.path().join("a.txt"), "two\n").unwrap();
    run(dir.path(), &["commit", "-am", "lane change"]);

    let metadata = inspect_git_worktree(dir.path(), Some(&base))
        .unwrap()
        .unwrap();

    assert_eq!(metadata.changed_files.unwrap(), vec!["a.txt"]);
}

#[test]
fn multiple_commits_report_union_since_base() {
    let dir = repo();
    let base = branch_head(dir.path());
    fs::write(dir.path().join("a.txt"), "two\n").unwrap();
    run(dir.path(), &["commit", "-am", "first"]);
    fs::write(dir.path().join("b.txt"), "bee\n").unwrap();
    run(dir.path(), &["add", "b.txt"]);
    run(dir.path(), &["commit", "-m", "second"]);

    let metadata = inspect_git_worktree(dir.path(), Some(&base))
        .unwrap()
        .unwrap();

    assert_eq!(metadata.changed_files.unwrap(), vec!["a.txt", "b.txt"]);
}

#[test]
fn dirty_overlay_dedupes_with_committed_diff() {
    let dir = repo();
    let base = branch_head(dir.path());
    fs::write(dir.path().join("a.txt"), "two\n").unwrap();
    run(dir.path(), &["commit", "-am", "lane change"]);
    fs::write(dir.path().join("a.txt"), "three\n").unwrap();

    let metadata = inspect_git_worktree(dir.path(), Some(&base))
        .unwrap()
        .unwrap();

    assert_eq!(metadata.changed_files.unwrap(), vec!["a.txt"]);
}

#[test]
fn invalid_base_is_refused() {
    let dir = repo();
    let err = inspect_git_worktree(dir.path(), Some("not-a-real-base"))
        .unwrap_err()
        .message();
    assert!(err.contains("invalid report base"));
}

#[test]
fn leading_dash_base_is_refused() {
    let dir = repo();
    let err = inspect_git_worktree(dir.path(), Some("--bad"))
        .unwrap_err()
        .message();
    assert!(err.contains("starting with '-'"));
}

#[test]
fn slash_ref_base_resolves_when_it_is_a_commit_ref() {
    let dir = repo();
    run(dir.path(), &["checkout", "-b", "lane/valid"]);
    fs::write(dir.path().join("a.txt"), "two\n").unwrap();
    run(dir.path(), &["commit", "-am", "lane change"]);
    let lane_head = branch_head(dir.path());
    run(dir.path(), &["checkout", "main"]);

    let metadata = inspect_git_worktree(dir.path(), Some("refs/heads/lane/valid"))
        .unwrap()
        .unwrap();

    assert_eq!(metadata.changed_files.unwrap(), vec!["a.txt"]);
    assert_ne!(metadata.sha.unwrap(), lane_head);
}

#[test]
fn stale_readiness_sha_is_refused() {
    let err = {
        let dir = repo();
        enrich_ready_report_in(
            dir.path(),
            Some(ReportStatusArg::InReview),
            None,
            Some("deadbeef".into()),
            None,
            None,
            false,
        )
        .unwrap_err()
    };

    assert!(err
        .message()
        .contains("provided --sha deadbeef does not match HEAD"));
    assert!(err.message().contains("--status partial"));
}

#[test]
fn wrong_branch_is_refused() {
    let dir = repo();

    let err = enrich_ready_report_in(
        dir.path(),
        Some(ReportStatusArg::InReview),
        Some("lane/wrong".into()),
        None,
        None,
        None,
        false,
    )
    .unwrap_err();

    assert!(err.message().contains("--branch"));
    assert!(err.message().contains("does not match current branch"));
}

#[test]
fn non_git_directory_is_ignored() {
    let dir = tempfile::tempdir().unwrap();

    assert_eq!(inspect_git_worktree(dir.path(), None).unwrap(), None);
}

#[test]
fn partial_report_does_not_inspect_git() {
    let out = enrich_ready_report(
        None,
        Some(ReportStatusArg::Partial),
        None,
        None,
        ReportBase::default(),
        Some(vec!["manual.rs".into()]),
        false,
    )
    .unwrap();

    assert_eq!(out.2, Some(vec!["manual.rs".into()]));
}

#[test]
fn concurrent_foreign_head_advance_accepts_owned_sha() {
    let dir = repo();
    let base = branch_head(dir.path());
    fs::write(dir.path().join("owned.txt"), "owned\n").unwrap();
    run(dir.path(), &["add", "owned.txt"]);
    run(dir.path(), &["commit", "-m", "owned"]);
    let owned = branch_head(dir.path());
    fs::write(dir.path().join("foreign.txt"), "foreign\n").unwrap();
    run(dir.path(), &["add", "foreign.txt"]);
    run(dir.path(), &["commit", "-m", "foreign"]);

    let (_, reported, _) = enrich_ready_report_in(
        dir.path(),
        Some(ReportStatusArg::InReview),
        None,
        Some(owned.clone()),
        Some(base),
        None,
        false,
    )
    .unwrap();

    assert_eq!(reported.as_deref(), Some(owned.as_str()));
}

#[test]
fn shared_checkout_ignores_dirty_file_outside_owned_commit_paths() {
    let dir = repo();
    let base = branch_head(dir.path());
    fs::write(dir.path().join("owned.txt"), "owned\n").unwrap();
    run(dir.path(), &["add", "owned.txt"]);
    run(dir.path(), &["commit", "-m", "owned"]);
    let owned = branch_head(dir.path());
    fs::write(dir.path().join("later.txt"), "foreign commit\n").unwrap();
    run(dir.path(), &["add", "later.txt"]);
    run(dir.path(), &["commit", "-m", "foreign"]);
    fs::write(dir.path().join("foreign.txt"), "foreign dirt\n").unwrap();

    let (_, reported, _) = enrich_ready_report_in(
        dir.path(),
        Some(ReportStatusArg::InReview),
        None,
        Some(owned.clone()),
        Some(base),
        None,
        false,
    )
    .expect("foreign dirty paths must not block the owning lane's report");

    assert_eq!(reported.as_deref(), Some(owned.as_str()));
}

#[test]
fn stale_sha_before_attempt_base_is_refused() {
    let dir = repo();
    let stale = branch_head(dir.path());
    fs::write(dir.path().join("base.txt"), "base\n").unwrap();
    run(dir.path(), &["add", "base.txt"]);
    run(dir.path(), &["commit", "-m", "attempt base"]);
    let base = branch_head(dir.path());
    fs::write(dir.path().join("owned.txt"), "owned\n").unwrap();
    run(dir.path(), &["add", "owned.txt"]);
    run(dir.path(), &["commit", "-m", "owned"]);

    for stale_or_base in [stale, base.clone()] {
        let err = enrich_ready_report_in(
            dir.path(),
            Some(ReportStatusArg::InReview),
            None,
            Some(stale_or_base),
            Some(base.clone()),
            None,
            false,
        )
        .unwrap_err();

        assert!(err.reason.contains("does not match HEAD"));
    }
}

#[test]
fn unrelated_branch_sha_is_refused() {
    let dir = repo();
    let base = branch_head(dir.path());
    run(dir.path(), &["checkout", "-b", "unrelated"]);
    fs::write(dir.path().join("unrelated.txt"), "unrelated\n").unwrap();
    run(dir.path(), &["add", "unrelated.txt"]);
    run(dir.path(), &["commit", "-m", "unrelated"]);
    let unrelated = branch_head(dir.path());
    run(dir.path(), &["checkout", "main"]);
    fs::write(dir.path().join("owned.txt"), "owned\n").unwrap();
    run(dir.path(), &["add", "owned.txt"]);
    run(dir.path(), &["commit", "-m", "owned"]);

    let err = enrich_ready_report_in(
        dir.path(),
        Some(ReportStatusArg::InReview),
        None,
        Some(unrelated),
        Some(base),
        None,
        false,
    )
    .unwrap_err();

    assert!(err.reason.contains("does not match HEAD"));
}

#[test]
fn dirty_owned_worktree_is_refused() {
    let dir = repo();
    let base = branch_head(dir.path());
    fs::write(dir.path().join("owned.txt"), "owned\n").unwrap();
    run(dir.path(), &["add", "owned.txt"]);
    run(dir.path(), &["commit", "-m", "owned"]);
    let owned = branch_head(dir.path());
    fs::write(dir.path().join("foreign.txt"), "foreign\n").unwrap();
    run(dir.path(), &["add", "foreign.txt"]);
    run(dir.path(), &["commit", "-m", "foreign"]);
    fs::write(dir.path().join("owned.txt"), "dirty owned\n").unwrap();

    let err = enrich_ready_report_in(
        dir.path(),
        Some(ReportStatusArg::InReview),
        None,
        Some(owned),
        Some(base),
        None,
        false,
    )
    .unwrap_err();

    assert_eq!(
        err.reason,
        "readiness report refused: this git worktree has uncommitted lane changes."
    );
    assert_eq!(err.dirty_files, vec!["owned.txt"]);
}

#[test]
fn isolated_lane_still_requires_exact_head() {
    let (_dir, _main, linked) = repo_with_linked_worktree();
    let base = branch_head(&linked);
    fs::write(linked.join("owned.txt"), "owned\n").unwrap();
    run(&linked, &["add", "owned.txt"]);
    run(&linked, &["commit", "-m", "owned"]);
    let owned = branch_head(&linked);
    fs::write(linked.join("later.txt"), "later\n").unwrap();
    run(&linked, &["add", "later.txt"]);
    run(&linked, &["commit", "-m", "later"]);

    let err = enrich_ready_report_in(
        &linked,
        Some(ReportStatusArg::InReview),
        None,
        Some(owned),
        Some(base),
        None,
        false,
    )
    .unwrap_err();

    assert!(err.reason.contains("does not match HEAD"));
}

fn lane_worktree_with_owned_commit() -> (TempDir, PathBuf, String, String) {
    let dir = repo();
    let lane = dir.path().join("lane");
    let lane_str = lane.to_string_lossy().into_owned();
    run(
        dir.path(),
        &["worktree", "add", "-b", "lane/report", &lane_str],
    );
    let base = branch_head(&lane);
    fs::write(lane.join("owned.txt"), "owned\n").unwrap();
    run(&lane, &["add", "owned.txt"]);
    run(&lane, &["commit", "-m", "owned"]);
    let owned = branch_head(&lane);
    (dir, lane, base, owned)
}

#[test]
fn scope_bound_lane_validates_against_lane_head_not_the_process_cwd() {
    let (_lane_dir, lane, base, owned) = lane_worktree_with_owned_commit();
    let scope = crate::cmd::report_lane::TaskReadinessScope {
        worktree_path: Some(lane),
        base: Some(base),
        requires_repository: true,
    };

    let (branch, sha, _changed) = enrich_ready_report(
        Some(&scope),
        Some(ReportStatusArg::InReview),
        None,
        Some(owned.clone()),
        ReportBase::default(),
        None,
        false,
    )
    .expect("a scope-bound lane worktree must validate the owned sha against its own HEAD");

    assert_eq!(sha.as_deref(), Some(owned.as_str()));
    assert_eq!(branch.as_deref(), Some("lane/report"));
}

#[test]
fn repository_scope_without_lane_worktree_refuses_instead_of_canonical() {
    let scope = crate::cmd::report_lane::TaskReadinessScope {
        worktree_path: None,
        base: None,
        requires_repository: true,
    };

    let err = enrich_ready_report(
        Some(&scope),
        Some(ReportStatusArg::InReview),
        None,
        Some("0123456789abcdef0123456789abcdef01234567".into()),
        ReportBase::default(),
        None,
        false,
    )
    .expect_err("an unresolved lane must refuse rather than validate against canonical HEAD");

    assert!(
        err.reason.contains("cannot locate your lane worktree"),
        "{}",
        err.reason
    );
}

#[test]
fn lane_owned_sha_validates_against_the_lane_worktree_not_the_pane_cwd() {
    let (_lane_dir, lane, base, owned) = lane_worktree_with_owned_commit();
    let foreign = repo();

    let (branch, sha, _changed) = enrich_ready_report_in_scope(
        ReadinessScope {
            pane_cwd: foreign.path(),
            lane_worktree: Some(&lane),
        },
        Some(ReportStatusArg::InReview),
        None,
        Some(owned.clone()),
        Some(base),
        None,
        false,
    )
    .expect("lane-owned sha and base must resolve in the lane worktree");

    assert_eq!(sha.as_deref(), Some(owned.as_str()));
    assert_eq!(branch.as_deref(), Some("lane/report"));
}

#[test]
fn dev_bound_lane_worktree_ignores_foreign_vision_outside_owned_interval() {
    let dir = repo();
    let lane = dir.path().join("lane");
    let lane_str = lane.to_string_lossy().into_owned();
    run(
        dir.path(),
        &["worktree", "add", "-b", "lane/report", &lane_str],
    );
    let base = branch_head(&lane);
    fs::write(lane.join("AGENTS.md"), "owned\n").unwrap();
    fs::create_dir_all(lane.join("evals/_lib")).unwrap();
    fs::write(lane.join("evals/_lib/lib.sh"), "owned\n").unwrap();
    run(&lane, &["add", "AGENTS.md", "evals/_lib/lib.sh"]);
    run(&lane, &["commit", "-m", "owned"]);
    let owned = branch_head(&lane);
    fs::write(lane.join("VISION.md"), "foreign dirt\n").unwrap();

    let (_, reported, _) = enrich_ready_report_in_scope(
        ReadinessScope {
            pane_cwd: dir.path(),
            lane_worktree: Some(&lane),
        },
        Some(ReportStatusArg::InReview),
        None,
        Some(owned.clone()),
        Some(base),
        None,
        false,
    )
    .expect("foreign VISION.md must not block the owning lane's report");

    assert_eq!(reported.as_deref(), Some(owned.as_str()));
}

#[test]
fn pane_cwd_head_is_never_accepted_as_the_lane_commit() {
    let (_lane_dir, lane, _base, _owned) = lane_worktree_with_owned_commit();
    let foreign = repo();
    let foreign_head = branch_head(foreign.path());

    let err = enrich_ready_report_in_scope(
        ReadinessScope {
            pane_cwd: foreign.path(),
            lane_worktree: Some(&lane),
        },
        Some(ReportStatusArg::InReview),
        None,
        Some(foreign_head),
        None,
        None,
        false,
    )
    .expect_err("a commit that exists only in the pane cwd must never satisfy a lane report");

    assert!(err.reason.contains("does not match HEAD"), "{}", err.reason);
}

#[cfg(unix)]
#[test]
fn lane_anchor_resolves_through_a_symlinked_recorded_path() {
    let (_lane_dir, lane, base, owned) = lane_worktree_with_owned_commit();
    let foreign = repo();
    let link_dir = tempfile::tempdir().unwrap();
    let link = link_dir.path().join("lane-link");
    std::os::unix::fs::symlink(&lane, &link).unwrap();

    let (branch, sha, _changed) = enrich_ready_report_in_scope(
        ReadinessScope {
            pane_cwd: foreign.path(),
            lane_worktree: Some(&link),
        },
        Some(ReportStatusArg::InReview),
        None,
        Some(owned.clone()),
        Some(base),
        None,
        false,
    )
    .expect("a symlinked lane path must resolve to the same lane worktree");

    assert_eq!(sha.as_deref(), Some(owned.as_str()));
    assert_eq!(branch.as_deref(), Some("lane/report"));
}

#[test]
fn declared_lane_worktree_that_is_not_a_repo_refuses_instead_of_using_the_pane() {
    let foreign = repo();
    let absent = foreign.path().join("no-such-lane");

    let err = enrich_ready_report_in_scope(
        ReadinessScope {
            pane_cwd: foreign.path(),
            lane_worktree: Some(&absent),
        },
        Some(ReportStatusArg::InReview),
        None,
        Some(branch_head(foreign.path())),
        None,
        None,
        false,
    )
    .expect_err("a missing lane worktree must not silently fall back to the pane checkout");

    assert!(
        err.reason.contains("is not a git worktree"),
        "{}",
        err.reason
    );
}

/// Mechanism only: the `--sha` argument itself must never be a shell
/// expression, since that could resolve to a foreign HEAD in a lane
/// worktree rather than the caller's own commit. Other legitimate `$(...)`
/// substitutions (e.g. `$(codeterm pad path)`) are untouched.
#[test]
fn refusal_never_advises_a_shell_expression_for_the_commit_sha() {
    let refusal = ReportRefusal {
        reason: "readiness report refused".into(),
        dirty_files: Vec::new(),
    };
    let message = refusal.message();

    assert!(!message.contains("--sha $("), "{message}");
    for line in REPORT_NEXT_COMMANDS {
        assert!(!line.contains("--sha $("), "{line}");
    }
}

/// codeterm#305: spawn and report derive the same verdict from the same
/// signals, so they cannot drift apart.
#[test]
fn spawn_and_report_agree_on_plan_binding_requirement() {
    // (has_plan_task, has_plan, has_task, expected bound)
    let scenarios = [
        (false, false, false, false),
        (false, true, false, false),
        (false, false, true, false),
        (false, true, true, true),
        (true, false, false, true),
        (true, true, true, true),
    ];
    for (has_plan_task, has_plan, has_task, expected) in scenarios {
        assert_eq!(
            worker_binding_resolved(has_plan_task, has_plan, has_task),
            expected,
            "spawn: plan_task={has_plan_task} plan={has_plan} task={has_task}"
        );
        if !has_plan_task {
            assert_eq!(
                worker_binding_resolved(false, has_plan, has_task),
                expected,
                "report: plan={has_plan} task={has_task}"
            );
        }
    }
}

/// codeterm#371: a live Planner reports plan-only and counts as bound.
#[test]
fn only_a_manager_role_is_exempt_from_the_task_half_of_binding() {
    assert!(role_owns_plan_without_task(Some("Manager")));
    assert!(!role_owns_plan_without_task(Some("Worker")));
    assert!(!role_owns_plan_without_task(None));
}
