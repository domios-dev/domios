//! Worker spawn guard: a Worker must not run in a canonical (primary) project
//! checkout — it gets an isolated git worktree, or an explicit exception.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::orchestrator::SpawnAgentParams;
use super::ServiceContext;
use crate::plan::AgentRef;
use crate::process_util;

/// A Worker spawn that was allowed into a shared/canonical checkout via
/// `--allow-shared-checkout`. Carries the binding needed to record the
/// visible exception stride once the spawn succeeds.
pub(crate) struct SharedCheckoutException {
    pub plan_id: Option<String>,
    pub task_id: Option<String>,
    pub cwd: String,
}

/// Only real Workers are guarded. Managers and Orchestrators are not Worker lanes.
pub(crate) fn is_worker_spawn(role: Option<&str>, _role_profile: Option<&str>) -> bool {
    role == Some("Worker")
}

/// A canonical checkout is one a Worker must not share: it equals the
/// workspace's registered primary cwd, or it is a primary git checkout
/// (`--git-dir` == `--git-common-dir`) rather than a linked worktree.
pub(crate) fn is_canonical_checkout(cwd: &str, workspace_cwd: Option<&str>) -> bool {
    if let Some(ws) = workspace_cwd {
        if same_path(cwd, ws) {
            return true;
        }
    }
    git_is_primary_checkout(Path::new(cwd))
}

fn same_path(a: &str, b: &str) -> bool {
    match (std::fs::canonicalize(a).ok(), std::fs::canonicalize(b).ok()) {
        (Some(a), Some(b)) => a == b,
        _ => ServiceContext::paths_equal(a, b),
    }
}

fn git_is_primary_checkout(cwd: &Path) -> bool {
    let mut cmd = Command::new("git");
    process_util::no_window_std(&mut cmd);
    let out = match cmd
        .current_dir(cwd)
        .args([
            "rev-parse",
            "--path-format=absolute",
            "--git-dir",
            "--git-common-dir",
        ])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return false,
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    match (lines.next(), lines.next()) {
        (Some(git_dir), Some(common_dir)) => norm(git_dir) == norm(common_dir),
        _ => false,
    }
}

fn norm(s: &str) -> PathBuf {
    std::fs::canonicalize(s).unwrap_or_else(|_| PathBuf::from(s))
}

pub(crate) fn refusal_message(cwd: &str) -> String {
    let repo = std::path::Path::new(cwd)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project");
    format!(
        "Refusing to spawn a Worker into the canonical checkout '{cwd}'. \
Workers get isolated worktrees in ONE sibling container `../{repo}-worktrees/` (never nested inside the repo):\n  \
git -C '{cwd}' worktree add ../{repo}-worktrees/<branch-slug> -b lane/<task-slug>\n\
then spawn with --cwd <that path>. Or pass --allow-shared-checkout to record an exception."
    )
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl ServiceContext {
    /// Evaluate the Worker-into-canonical-checkout guard before spawning.
    /// `Ok(None)` → not guarded / allowed silently. `Ok(Some(_))` → allowed via
    /// exception, caller records the stride after a successful spawn.
    /// `Err(_)` → refused with the fix text.
    pub(crate) fn worker_checkout_guard(
        &self,
        params: &SpawnAgentParams,
    ) -> Result<Option<SharedCheckoutException>, String> {
        if !is_worker_spawn(params.role.as_deref(), params.role_profile.as_deref()) {
            return Ok(None);
        }
        let ws_cwd = self.resolve_spawn_workspace_cwd(params);
        let effective = match params.cwd.clone().or_else(|| ws_cwd.clone()) {
            Some(dir) => dir,
            None => return Ok(None),
        };
        if !is_canonical_checkout(&effective, ws_cwd.as_deref()) {
            return Ok(None);
        }
        if !params.allow_shared_checkout.unwrap_or(false) {
            return Err(refusal_message(&effective));
        }
        Ok(Some(SharedCheckoutException {
            plan_id: params.plan_id.clone(),
            task_id: params.plan_task.clone().or_else(|| params.task_id.clone()),
            cwd: effective,
        }))
    }

    fn resolve_spawn_workspace_cwd(&self, params: &SpawnAgentParams) -> Option<String> {
        let mux = self.mux.read();
        let id = crate::service::spawn::resolve_spawn_workspace_id(
            params.workspace_id.clone(),
            params.parent_pane_id.as_deref(),
            |pid| mux.find_workspace_for_pane(pid),
            mux.get_active_workspace().map(|w| w.id.clone()),
            mux.list_workspaces().first().map(|w| w.id.clone()),
        )
        .ok()?;
        mux.get_workspace(&id).and_then(|ws| ws.cwd.clone())
    }

    /// Record the shared-checkout exception as a visible stride on the bound
    /// task (never silent). Best-effort: without a plan+task binding it still
    /// logs a warning so no shared-checkout Worker is invisible.
    pub(crate) fn record_shared_checkout_exception(
        &self,
        exc: &SharedCheckoutException,
        pane_id: &str,
    ) {
        let text = format!(
            "Shared-checkout exception: Worker spawned into canonical checkout '{}' via --allow-shared-checkout (pane {pane_id})",
            exc.cwd
        );
        log::warn!("[spawn-guard] {text}");
        if let (Some(plan_id), Some(task_id)) = (exc.plan_id.as_deref(), exc.task_id.as_deref()) {
            if let Err(e) = self.plan_store.add_milestone(
                plan_id,
                task_id,
                "exception",
                &text,
                AgentRef::from_pane(pane_id),
                true,
                epoch_ms(),
            ) {
                log::warn!("[spawn-guard] failed to record exception stride: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let mut cmd = Command::new("git");
        crate::process_util::no_window_std(&mut cmd);
        let status = cmd.current_dir(dir).args(args).status().expect("git runs");
        assert!(status.success(), "git {:?} failed", args);
    }

    fn init_repo(dir: &Path) {
        git(dir, &["init", "-q"]);
        git(dir, &["config", "user.email", "t@t.t"]);
        git(dir, &["config", "user.name", "t"]);
        std::fs::write(dir.join("f.txt"), "x").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "init"]);
    }

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn worker_role_matrix() {
        assert!(is_worker_spawn(Some("Worker"), None));
        assert!(is_worker_spawn(Some("Worker"), Some("something")));
        assert!(is_worker_spawn(Some("Worker"), Some("planner")));
        assert!(!is_worker_spawn(Some("Manager"), Some("planner")));
        assert!(!is_worker_spawn(Some("Orchestrator"), None));
        assert!(!is_worker_spawn(None, None));
    }

    #[test]
    fn primary_checkout_is_canonical_and_worktree_is_not() {
        let tmp = tempfile::tempdir().unwrap();
        let primary = tmp.path().join("primary");
        std::fs::create_dir_all(&primary).unwrap();
        init_repo(&primary);

        let worktree = tmp.path().join("wt");
        git(
            &primary,
            &[
                "worktree",
                "add",
                "-q",
                worktree.to_str().unwrap(),
                "-b",
                "lane/x",
            ],
        );

        // Primary checkout: refused (canonical), with or without a workspace cwd.
        assert!(git_is_primary_checkout(&primary));
        assert!(is_canonical_checkout(&s(&primary), None));
        assert!(is_canonical_checkout(&s(&primary), Some(&s(&primary))));

        // Linked worktree distinct from the workspace cwd: allowed.
        assert!(!git_is_primary_checkout(&worktree));
        assert!(!is_canonical_checkout(&s(&worktree), Some(&s(&primary))));
    }

    #[test]
    fn sharing_the_workspace_dir_is_canonical_even_for_a_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let primary = tmp.path().join("primary");
        std::fs::create_dir_all(&primary).unwrap();
        init_repo(&primary);
        let worktree = tmp.path().join("wt");
        git(
            &primary,
            &[
                "worktree",
                "add",
                "-q",
                worktree.to_str().unwrap(),
                "-b",
                "lane/y",
            ],
        );
        // cwd == workspace cwd (the same worktree) => shared checkout => canonical.
        assert!(is_canonical_checkout(&s(&worktree), Some(&s(&worktree))));
    }

    #[test]
    fn non_git_unrelated_dir_is_not_canonical() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().join("plain");
        let ws = tmp.path().join("ws");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&ws).unwrap();
        assert!(!is_canonical_checkout(&s(&cwd), Some(&s(&ws))));
    }
}
