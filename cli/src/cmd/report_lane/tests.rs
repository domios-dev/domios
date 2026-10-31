use super::*;
use crate::args::agent::ReportStatusArg;
use crate::cmd::report_attribution::ReportBase;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::process::Command;
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
    fs::write(dir.path().join("README.md"), "initial\n").unwrap();
    run(dir.path(), &["add", "README.md"]);
    run(dir.path(), &["commit", "-m", "initial"]);
    dir
}

fn head(dir: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn lane_with_owned_commit() -> (TempDir, PathBuf, String, String) {
    let dir = repo();
    let lane = dir.path().join("lane");
    let lane_str = lane.to_string_lossy().into_owned();
    run(
        dir.path(),
        &["worktree", "add", "-b", "lane/report", &lane_str],
    );
    let base = head(&lane);
    let owned = lane.join("cli/src/cmd/report_guard.rs");
    fs::create_dir_all(owned.parent().unwrap()).unwrap();
    fs::write(&owned, "owned\n").unwrap();
    run(&lane, &["add", "cli/src/cmd/report_guard.rs"]);
    run(&lane, &["commit", "-m", "owned"]);
    let owned_sha = head(&lane);
    (dir, lane, base, owned_sha)
}

#[test]
fn falls_back_to_the_task_lane_worktree() {
    let task = json!({
        "id": "task_a",
        "lane": { "worktree_path": "/lanes/a", "base": "base-a" },
    });
    let scope = task_readiness_scope_from_task(&task, "tab-a");

    assert_eq!(scope.worktree_path.as_deref(), Some(Path::new("/lanes/a")));
    assert_eq!(scope.base.as_deref(), Some("base-a"));
    assert!(scope.requires_repository);
}

#[test]
fn strips_the_windows_verbatim_prefix_from_a_recorded_lane() {
    let task = json!({
        "id": "task_a",
        "lane": { "worktree_path": r"\\?\D:\Developer\codeterm-worktrees\pw15-speed" },
    });
    let scope = task_readiness_scope_from_task(&task, "tab-a");

    assert_eq!(
        scope.worktree_path.as_deref(),
        Some(Path::new(r"D:\Developer\codeterm-worktrees\pw15-speed")),
        "a verbatim-prefixed lane path is rejected by git as a working directory"
    );
}

#[test]
fn only_explicit_nonrepository_intent_skips_repository_guards() {
    let nonrepo =
        task_readiness_scope_from_task(&json!({ "execution_intent": "non_repository" }), "tab-a");
    let malformed =
        task_readiness_scope_from_task(&json!({ "execution_intent": "read_only" }), "tab-a");

    assert!(!nonrepo.requires_repository);
    assert!(malformed.requires_repository);
    assert!(TaskReadinessScope::default().requires_repository);
}

#[test]
fn open_attempt_worktree_and_base_win_as_one_scope() {
    let task = json!({
        "id": "task_a",
        "lane": { "worktree_path": "/lanes/stale", "base": "stale-base" },
        "execution_attempts": [
            {
                "executor_tab_id": "tab-a",
                "worktree_path": "/lanes/live",
                "base": "live-base"
            },
        ],
    });
    let scope = task_readiness_scope_from_task(&task, "tab-a");

    assert_eq!(
        scope.worktree_path.as_deref(),
        Some(Path::new("/lanes/live"))
    );
    assert_eq!(scope.base.as_deref(), Some("live-base"));
}

#[test]
fn open_attempt_never_borrows_a_missing_base_from_the_lane() {
    let task = json!({
        "id": "task_a",
        "lane": { "worktree_path": "/lanes/stale", "base": "stale-base" },
        "execution_attempts": [
            { "executor_tab_id": "tab-a", "worktree_path": "/lanes/live" },
        ],
    });
    let scope = task_readiness_scope_from_task(&task, "tab-a");

    assert_eq!(
        scope.worktree_path.as_deref(),
        Some(Path::new("/lanes/live"))
    );
    assert_eq!(scope.base, None);
}

#[test]
fn closed_and_foreign_attempts_are_ignored() {
    let task = json!({
        "id": "task_a",
        "lane": { "worktree_path": "/lanes/a", "base": "base-a" },
        "execution_attempts": [
            {
                "executor_tab_id": "tab-a",
                "worktree_path": "/lanes/closed",
                "base": "closed-base",
                "closed_at": "t1"
            },
            {
                "executor_tab_id": "tab-b",
                "worktree_path": "/lanes/other",
                "base": "other-base"
            },
        ],
    });
    let scope = task_readiness_scope_from_task(&task, "tab-a");

    assert_eq!(scope.worktree_path.as_deref(), Some(Path::new("/lanes/a")));
    assert_eq!(scope.base.as_deref(), Some("base-a"));
}

#[test]
fn open_attempt_without_worktree_falls_back_to_task_lane() {
    let task = json!({
        "id": "task_a",
        "lane": { "worktree_path": "/lanes/a", "base": "base-a" },
        "execution_attempts": [
            { "executor_tab_id": "tab-a", "base": "attempt-base" },
        ],
    });
    let scope = task_readiness_scope_from_task(&task, "tab-a");

    assert_eq!(scope.worktree_path.as_deref(), Some(Path::new("/lanes/a")));
    assert_eq!(scope.base.as_deref(), Some("base-a"));
}

#[test]
fn absent_lane_worktree_resolves_to_none() {
    let task = json!({ "id": "task_a", "lane": { "worktree_path": "  " } });
    let scope = task_readiness_scope_from_task(&task, "tab-a");

    assert_eq!(scope.worktree_path, None);
    assert_eq!(scope.base, None);
}

#[test]
fn canonical_writer_in_review_report_with_sha_is_validated_against_canonical() {
    let dir = repo();
    fs::write(dir.path().join("owned.txt"), "canonical work\n").unwrap();
    run(dir.path(), &["add", "owned.txt"]);
    run(dir.path(), &["commit", "-m", "canonical work"]);
    let committed = head(dir.path());
    let task = json!({
        "id": "task_a",
        "lane": {},
        "execution_attempts": [{
            "executor_tab_id": "tab-a",
            "placement": "canonical_writer"
        }],
    });
    let plan = json!({
        "canonical_project_path": dir.path().to_string_lossy(),
        "tasks": [task],
    });
    let scope = task_readiness_scope_from_plan(&plan, "task_a", "tab-a")
        .expect("the canonical writer attempt belongs to the plan");

    assert!(scope.canonical_writer);
    assert_eq!(scope.worktree_path, None);
    assert_eq!(scope.canonical_checkout_path.as_deref(), Some(dir.path()));

    let (branch, reported, _) = crate::cmd::report_guard::enrich_ready_report(
        Some(&scope),
        Some(ReportStatusArg::InReview),
        None,
        Some(committed.clone()),
        ReportBase::default(),
        None,
        false,
    )
    .expect("the attempt placement must bind the report to the plan's canonical checkout");

    assert_eq!(branch.as_deref(), Some("main"));
    assert_eq!(reported.as_deref(), Some(committed.as_str()));
}

#[test]
fn canonical_writer_report_with_dirty_canonical_is_refused() {
    let dir = repo();
    let committed = head(dir.path());
    let task = json!({
        "id": "task_a",
        "lane": {},
        "execution_attempts": [{
            "executor_tab_id": "tab-a",
            "placement": "canonical_writer"
        }],
    });
    let plan = json!({
        "canonical_project_path": dir.path().to_string_lossy(),
        "tasks": [task],
    });
    let scope = task_readiness_scope_from_plan(&plan, "task_a", "tab-a")
        .expect("the canonical writer attempt belongs to the plan");
    fs::write(dir.path().join("VISION.md"), "uncommitted\n").unwrap();

    let error = crate::cmd::report_guard::enrich_ready_report(
        Some(&scope),
        Some(ReportStatusArg::InReview),
        None,
        Some(committed.clone()),
        ReportBase::typed(&committed),
        None,
        true,
    )
    .expect_err("canonical dirt must refuse despite explicit base and dirt acknowledgement");

    assert_eq!(error.dirty_files, vec!["VISION.md"]);
}

#[test]
fn lane_bound_report_without_worktree_still_refuses() {
    let dir = repo();
    let task = json!({
        "id": "task_a",
        "lane": {},
        "execution_attempts": [{
            "executor_tab_id": "tab-a"
        }],
    });
    let plan = json!({
        "canonical_project_path": dir.path().to_string_lossy(),
        "tasks": [task],
    });
    let scope = task_readiness_scope_from_plan(&plan, "task_a", "tab-a")
        .expect("the lane-bound attempt belongs to the plan");

    let error = crate::cmd::report_guard::enrich_ready_report(
        Some(&scope),
        Some(ReportStatusArg::InReview),
        None,
        Some(head(dir.path())),
        ReportBase::default(),
        None,
        false,
    )
    .expect_err("canonical path availability must not resolve an isolated lane");

    assert!(!scope.canonical_writer);
    assert!(error.reason.contains("cannot locate your lane worktree"));
    assert_eq!(
        error.report_outcome(),
        Some(codeterm_types::agent::ReportOutcomeClass::DeliveredUnattached)
    );
}

#[test]
fn task_is_found_by_handle() {
    let plan = json!({ "tasks": [{ "id": "task_a", "handle": "alpha" }] });

    assert_eq!(find_task(&plan, "alpha"), Some(&plan["tasks"][0]));
    assert_eq!(find_task(&plan, "missing"), None);
}

#[test]
fn task_base_reaches_report_guard_when_worker_environment_has_no_base() {
    let (_dir, lane, base, owned_sha) = lane_with_owned_commit();
    let lane_str = lane.to_string_lossy().into_owned();
    fs::write(lane.join("VISION.md"), "foreign dirt\n").unwrap();
    let task = json!({
        "id": "task_a",
        "lane": { "worktree_path": lane_str, "base": base },
    });
    let task_scope = task_readiness_scope_from_task(&task, "tab-a");

    let (_, reported, _) = crate::cmd::report_guard::enrich_ready_report(
        Some(&task_scope),
        Some(ReportStatusArg::InReview),
        None,
        Some(owned_sha.clone()),
        ReportBase::default(),
        None,
        false,
    )
    .expect("the task-derived base must scope foreign VISION.md out of lane dirt");

    assert_eq!(reported.as_deref(), Some(owned_sha.as_str()));
}

#[test]
fn task_scope_without_base_keeps_dirty_report_strict() {
    let (_dir, lane, _base, owned_sha) = lane_with_owned_commit();
    fs::write(lane.join("VISION.md"), "unowned dirt\n").unwrap();
    let task = json!({
        "id": "task_a",
        "lane": { "worktree_path": lane.to_string_lossy() },
    });
    let task_scope = task_readiness_scope_from_task(&task, "tab-a");

    let error = crate::cmd::report_guard::enrich_ready_report(
        Some(&task_scope),
        Some(ReportStatusArg::InReview),
        None,
        Some(owned_sha),
        ReportBase::default(),
        None,
        false,
    )
    .expect_err("missing task base must keep the dirty gate strict");

    assert_eq!(error.dirty_files, vec!["VISION.md"]);
}

#[test]
fn task_scope_still_refuses_dirty_owned_path() {
    let (_dir, lane, base, owned_sha) = lane_with_owned_commit();
    fs::write(lane.join("cli/src/cmd/report_guard.rs"), "dirty owned\n").unwrap();
    let task = json!({
        "id": "task_a",
        "lane": { "worktree_path": lane.to_string_lossy(), "base": base },
    });
    let task_scope = task_readiness_scope_from_task(&task, "tab-a");

    let error = crate::cmd::report_guard::enrich_ready_report(
        Some(&task_scope),
        Some(ReportStatusArg::InReview),
        None,
        Some(owned_sha),
        ReportBase::default(),
        None,
        false,
    )
    .expect_err("dirty owned paths must still block readiness");

    assert_eq!(error.dirty_files, vec!["cli/src/cmd/report_guard.rs"]);
}

#[test]
fn canonical_writer_attempt_binds_report_to_canonical_with_strict_dirt() {
    let dir = repo();
    let task = json!({
        "id": "task_a",
        "lane": {},
        "execution_attempts": [{
            "executor_tab_id": "tab-a",
            "worktree_path": dir.path().to_string_lossy(),
            "placement": "canonical_writer"
        }],
    });
    let scope = task_readiness_scope_from_task(&task, "tab-a");
    assert_eq!(scope.worktree_path.as_deref(), Some(dir.path()));
    assert_eq!(scope.base, None);

    let committed = head(dir.path());
    fs::write(dir.path().join("VISION.md"), "uncommitted\n").unwrap();
    let error = crate::cmd::report_guard::enrich_ready_report(
        Some(&scope),
        Some(ReportStatusArg::InReview),
        None,
        Some(committed.clone()),
        ReportBase::default(),
        None,
        false,
    )
    .expect_err("any dirt in canonical refuses a canonical writer report");
    assert_eq!(error.dirty_files, vec!["VISION.md"]);

    fs::remove_file(dir.path().join("VISION.md")).unwrap();
    let (_, reported, _) = crate::cmd::report_guard::enrich_ready_report(
        Some(&scope),
        Some(ReportStatusArg::InReview),
        None,
        Some(committed.clone()),
        ReportBase::default(),
        None,
        false,
    )
    .expect("a clean canonical writer commit reports");
    assert_eq!(reported.as_deref(), Some(committed.as_str()));
}

#[test]
fn report_binding_model_uses_only_current_recorded_paths() {
    let cases = [
        (
            json!({"lane": {"worktree_path": "/lanes/a"}}),
            Some("/lanes/a"),
            false,
        ),
        (
            json!({"execution_attempts": [{
                "executor_tab_id": "tab-a", "checkout_path": "/integration"
            }]}),
            Some("/integration"),
            true,
        ),
        (json!({}), None, false),
        (
            json!({"execution_attempts": [{
                "executor_tab_id": "tab-other", "checkout_path": "/foreign"
            }]}),
            None,
            false,
        ),
        (
            json!({"execution_attempts": [{
                "executor_tab_id": "tab-a", "checkout_path": "/closed", "closed_at": 1
            }]}),
            None,
            false,
        ),
        (
            json!({"execution_attempts": [{
                "executor_tab_id": "tab-a", "checkout_path": "/retired", "retired_at": 1
            }]}),
            None,
            false,
        ),
        (
            json!({"execution_attempts": [{
                "executor_tab_id": "tab-a", "checkout_path": "  "
            }]}),
            None,
            false,
        ),
        (
            json!({
                "lane": {"worktree_path": "/stale"},
                "execution_attempts": [{
                    "executor_tab_id": "tab-a", "checkout_path": "/integration"
                }]
            }),
            Some("/integration"),
            true,
        ),
    ];
    for (task, expected_path, recorded_checkout) in cases {
        let scope =
            task_readiness_scope_from_task_with_canonical_path(&task, "tab-a", Some("/canonical"));
        assert_eq!(scope.worktree_path.as_deref(), expected_path.map(Path::new));
        assert_eq!(scope.recorded_checkout, recorded_checkout);
        assert_eq!(scope.canonical_checkout_path, None);
        assert!(!scope.canonical_writer);
    }
}

#[test]
fn serialized_attempt_retains_checkout_report_binding() {
    let attempt = codeterm_types::plan::TaskExecutionAttempt {
        id: "attempt-a".into(),
        executor_tab_id: "tab-a".into(),
        checkout_path: Some("/integration".into()),
        ..Default::default()
    };
    let bytes = serde_json::to_vec(&attempt).unwrap();
    let restored: codeterm_types::plan::TaskExecutionAttempt =
        serde_json::from_slice(&bytes).unwrap();
    assert_eq!(restored, attempt);
    let task = json!({"execution_attempts": [restored]});
    let scope = task_readiness_scope_from_task(&task, "tab-a");
    assert_eq!(
        scope.worktree_path.as_deref(),
        Some(Path::new("/integration"))
    );
    assert!(scope.recorded_checkout);
}

struct ScopeLookupRpc {
    reply: Option<Result<Value, String>>,
}

impl RpcClient for ScopeLookupRpc {
    fn call_detailed(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, crate::client::SocketCallError> {
        assert_eq!(method, "plan.get");
        assert_eq!(params.unwrap()["plan_id"], "plan-a");
        self.reply
            .take()
            .expect("unexpected scope lookup")
            .map_err(crate::client::SocketCallError::Transport)
    }
}

#[test]
fn unavailable_bound_scope_never_enters_checkout_inspection() {
    let lookups = [
        Err("transport unavailable".to_string()),
        Ok(json!(null)),
        Ok(json!({"tasks": {}})),
        Ok(json!({"tasks": []})),
        Ok(json!({"tasks": [{"id": "foreign", "execution_intent": "non_repository"}]})),
        Ok(json!({"tasks": [{"id": "task-a", "execution_intent": {}}]})),
        Ok(json!({"tasks": [{"id": "task-a", "execution_attempts": {}}]})),
    ];
    for lookup in lookups {
        let mut rpc = ScopeLookupRpc {
            reply: Some(lookup),
        };
        let scope = task_readiness_scope(&mut rpc, Some("plan-a"), Some("task-a"), "tab-a");
        let mut entered_guard = false;
        let result = scope.and_then(|scope| {
            entered_guard = true;
            crate::cmd::report_guard::enrich_ready_report(
                scope.as_ref(),
                Some(ReportStatusArg::InReview),
                None,
                None,
                ReportBase::default(),
                None,
                false,
            )
        });
        assert!(!entered_guard);
        let refusal = result.unwrap_err();
        assert!(refusal.reason.contains("binding is unavailable"));
        assert!(!refusal.message().contains("--status in_review"));
    }
    let mut rpc = ScopeLookupRpc { reply: None };
    assert!(task_readiness_scope(&mut rpc, None, Some("task-a"), "tab-a").is_err());
}

#[test]
fn taskless_and_authoritative_nonrepository_scopes_are_deliberate() {
    let mut rpc = ScopeLookupRpc { reply: None };
    assert_eq!(
        task_readiness_scope(&mut rpc, Some("plan-a"), None, "tab-a").unwrap(),
        None
    );
    rpc.reply = Some(Ok(json!({
        "tasks": [{"id": "task-a", "execution_intent": "non_repository"}]
    })));
    let scope = task_readiness_scope(&mut rpc, Some("plan-a"), Some("task-a"), "tab-a")
        .unwrap()
        .unwrap();
    let fields = crate::cmd::report_guard::enrich_ready_report(
        Some(&scope),
        Some(ReportStatusArg::InReview),
        None,
        Some("opaque-nonrepository-result".into()),
        ReportBase::default(),
        None,
        false,
    )
    .unwrap();
    assert_eq!(fields.1.as_deref(), Some("opaque-nonrepository-result"));
}
