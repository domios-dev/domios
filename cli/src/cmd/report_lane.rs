use std::path::PathBuf;

use serde_json::Value;

use crate::client::SocketClient;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskReadinessScope {
    pub worktree_path: Option<PathBuf>,
    pub canonical_writer: bool,
    pub canonical_checkout_path: Option<PathBuf>,
    pub base: Option<String>,
    pub requires_repository: bool,
}

pub fn task_readiness_scope(
    client: &mut SocketClient,
    plan_id: Option<&str>,
    task_id: Option<&str>,
    tab_id: &str,
) -> Option<TaskReadinessScope> {
    let plan_id = plan_id?;
    let task_id = task_id?;
    let plan = crate::cmd::plan::call_plan_rpc(
        client,
        "plan.get",
        serde_json::json!({ "plan_id": plan_id }),
    )
    .ok()?;
    task_readiness_scope_from_plan(&plan, task_id, tab_id)
}

fn task_readiness_scope_from_plan(
    plan: &Value,
    task_id: &str,
    tab_id: &str,
) -> Option<TaskReadinessScope> {
    let task = find_task(plan, task_id)?;
    let canonical_project_path = plan
        .get("canonical_project_path")
        .and_then(Value::as_str)
        .filter(|path| !path.trim().is_empty())
        .or_else(|| plan.get("project_path").and_then(Value::as_str));
    Some(task_readiness_scope_from_task_with_canonical_path(
        task,
        tab_id,
        canonical_project_path,
    ))
}

fn find_task<'a>(plan: &'a Value, task_id: &str) -> Option<&'a Value> {
    plan.get("tasks")?
        .as_array()?
        .iter()
        .find(|task| matches(task.get("id"), task_id) || matches(task.get("handle"), task_id))
}

fn matches(value: Option<&Value>, expected: &str) -> bool {
    value.and_then(Value::as_str) == Some(expected)
}

#[cfg(test)]
fn task_readiness_scope_from_task(task: &Value, tab_id: &str) -> TaskReadinessScope {
    task_readiness_scope_from_task_with_canonical_path(task, tab_id, None)
}

fn task_readiness_scope_from_task_with_canonical_path(
    task: &Value,
    tab_id: &str,
    canonical_project_path: Option<&str>,
) -> TaskReadinessScope {
    let attempt = open_attempt(task, tab_id);
    let canonical_writer = attempt
        .and_then(|attempt| attempt.get("placement"))
        .and_then(Value::as_str)
        == Some("canonical_writer");
    let lane = task.get("lane");
    // Use one attempt record for worktree and base so a live attempt cannot borrow a stale base from the lane.
    let record = match attempt {
        Some(attempt) if nonempty(attempt.get("worktree_path")).is_some() => Some(attempt),
        _ => lane.or(attempt),
    };
    let canonical_checkout_path = if canonical_writer {
        canonical_project_path
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(str::to_string)
            .or_else(|| attempt.and_then(|attempt| nonempty(attempt.get("worktree_path"))))
            .map(|path| PathBuf::from(codeterm_paths::strip_verbatim_prefix(&path)))
    } else {
        None
    };
    TaskReadinessScope {
        // Lane records written before the provisioning fix carry a Windows
        // verbatim prefix, which git rejects as a working directory.
        worktree_path: record
            .and_then(|record| nonempty(record.get("worktree_path")))
            .map(|path| PathBuf::from(codeterm_paths::strip_verbatim_prefix(&path))),
        canonical_writer,
        canonical_checkout_path,
        base: record.and_then(|record| nonempty(record.get("base"))),
        requires_repository: task.get("execution_intent").and_then(Value::as_str)
            != Some("non_repository"),
    }
}

impl Default for TaskReadinessScope {
    fn default() -> Self {
        Self {
            worktree_path: None,
            canonical_writer: false,
            canonical_checkout_path: None,
            base: None,
            requires_repository: true,
        }
    }
}

fn open_attempt<'a>(task: &'a Value, tab_id: &str) -> Option<&'a Value> {
    task.get("execution_attempts")?
        .as_array()?
        .iter()
        .rev()
        .find(|attempt| {
            matches(attempt.get("executor_tab_id"), tab_id)
                && attempt.get("closed_at").is_none_or(Value::is_null)
        })
}

fn nonempty(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
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
        let nonrepo = task_readiness_scope_from_task(
            &json!({ "execution_intent": "non_repository" }),
            "tab-a",
        );
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
    fn explicit_base_overrides_task_derived_fallback() {
        let (_dir, lane, base, owned_sha) = lane_with_owned_commit();
        fs::write(lane.join("VISION.md"), "foreign dirt\n").unwrap();
        let task_scope = TaskReadinessScope {
            worktree_path: Some(lane),
            base: Some("stale-task-base".into()),
            requires_repository: true,
            ..TaskReadinessScope::default()
        };

        let (_, reported, _) = crate::cmd::report_guard::enrich_ready_report(
            Some(&task_scope),
            Some(ReportStatusArg::InReview),
            None,
            Some(owned_sha.clone()),
            ReportBase::classify(Some(base), None),
            None,
            false,
        )
        .expect("explicit base must override the task-derived fallback");

        assert_eq!(reported.as_deref(), Some(owned_sha.as_str()));
    }
}
