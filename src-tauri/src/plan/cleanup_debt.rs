use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CleanupDebtKind {
    PendingStatus,
    DeferredStatus,
    LiveTab,
    WorktreePresent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CleanupDebtIssue {
    pub kind: CleanupDebtKind,
    pub detail: String,
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CleanupDebtTask {
    pub task_id: String,
    pub task_name: String,
    pub task_status: String,
    pub cleanup_status: Option<String>,
    pub deferred: bool,
    pub issues: Vec<CleanupDebtIssue>,
    pub commands: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CleanupDebtInput<'a> {
    pub plan_id: &'a str,
    pub task_id: &'a str,
    pub task_name: &'a str,
    pub task_status: &'a str,
    pub is_terminal: bool,
    pub cleanup_status: Option<&'a str>,
    pub cleanup_hints: &'a [String],
    pub tab_id: Option<&'a str>,
    pub tab_live: bool,
    pub worktree_path: Option<&'a str>,
    pub worktree_exists: bool,
    pub branch: Option<&'a str>,
    pub project_path: &'a str,
}

pub fn detect_cleanup_debt(input: &CleanupDebtInput<'_>) -> Option<CleanupDebtTask> {
    if !input.is_terminal {
        return None;
    }
    let cleanup_status = normalized(input.cleanup_status);
    let deferred = cleanup_status
        .as_deref()
        .is_some_and(|status| status.eq_ignore_ascii_case("deferred"));
    let done = cleanup_status
        .as_deref()
        .is_some_and(|status| status.eq_ignore_ascii_case("done"));
    let tab_id = normalized(input.tab_id);
    let worktree_path = normalized(input.worktree_path);
    let branch = normalized(input.branch);
    let has_cleanup_hints = input
        .cleanup_hints
        .iter()
        .any(|hint| !hint.trim().is_empty());
    let has_lane_residue =
        tab_id.is_some() || worktree_path.is_some() || branch.is_some() || has_cleanup_hints;

    let mut issues = Vec::new();
    if deferred {
        issues.push(CleanupDebtIssue {
            kind: CleanupDebtKind::DeferredStatus,
            detail: deferred_detail(input.cleanup_hints),
            command: Some(mark_done_command(input.plan_id, input.task_id)),
        });
    } else if !done && has_lane_residue {
        issues.push(CleanupDebtIssue {
            kind: CleanupDebtKind::PendingStatus,
            detail: "terminal task still has cleanup outstanding".into(),
            command: Some(mark_done_command(input.plan_id, input.task_id)),
        });
    }
    if input.tab_live {
        if let Some(tab_id) = tab_id.as_deref() {
            issues.push(CleanupDebtIssue {
                kind: CleanupDebtKind::LiveTab,
                detail: format!("worker tab {tab_id} is still live"),
                command: Some(format!("codeterm tab close {}", shell_quote(tab_id))),
            });
        }
    }
    if input.worktree_exists {
        if let Some(worktree_path) = worktree_path.as_deref() {
            issues.push(CleanupDebtIssue {
                kind: CleanupDebtKind::WorktreePresent,
                detail: format!("worktree still exists at {worktree_path}"),
                command: Some(remove_worktree_command(input.project_path, worktree_path)),
            });
        }
    }
    if issues.is_empty() {
        return None;
    }

    let mut commands = issues
        .iter()
        .filter_map(|issue| issue.command.clone())
        .collect::<Vec<_>>();
    if input.worktree_exists {
        if let Some(branch) = branch.as_deref() {
            commands.push(delete_branch_command(input.project_path, branch));
        }
    }
    if !done && !deferred {
        commands.push(mark_deferred_command(input.plan_id, input.task_id));
    }
    dedup(&mut commands);

    Some(CleanupDebtTask {
        task_id: input.task_id.to_string(),
        task_name: input.task_name.to_string(),
        task_status: input.task_status.to_string(),
        cleanup_status,
        deferred,
        issues,
        commands,
    })
}

#[allow(dead_code)]
pub fn render_cleanup_debt_hint(item: &CleanupDebtTask) -> String {
    let intro = if item.deferred {
        format!(
            "WARNING: cleanup deferred for {} ({})",
            item.task_id, item.task_name
        )
    } else {
        format!(
            "WARNING: cleanup debt for {} ({})",
            item.task_id, item.task_name
        )
    };
    let detail = item
        .issues
        .iter()
        .map(|issue| issue.detail.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    let commands = item.commands.join(" ; ");
    format!("{intro}: {detail}. Commands: {commands}")
}

fn deferred_detail(cleanup_hints: &[String]) -> String {
    let reason = cleanup_hints
        .iter()
        .map(|hint| hint.trim())
        .find(|hint| !hint.is_empty());
    match reason {
        Some(reason) => format!("cleanup is explicitly deferred: {reason}"),
        None => "cleanup is explicitly deferred".into(),
    }
}

fn mark_done_command(plan_id: &str, task_id: &str) -> String {
    format!(
        "codeterm plan task update {} {} --cleanup-status done",
        shell_quote(plan_id),
        shell_quote(task_id)
    )
}

fn mark_deferred_command(plan_id: &str, task_id: &str) -> String {
    format!(
        "codeterm plan task update {} {} --cleanup-status deferred --cleanup-hint \"<why>\"",
        shell_quote(plan_id),
        shell_quote(task_id)
    )
}

fn remove_worktree_command(project_path: &str, worktree_path: &str) -> String {
    format!(
        "git -C {} worktree remove {}",
        shell_quote(project_path),
        shell_quote(worktree_path)
    )
}

fn delete_branch_command(project_path: &str, branch: &str) -> String {
    format!(
        "git -C {} branch -D {}",
        shell_quote(project_path),
        shell_quote(branch)
    )
}

fn normalized(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn dedup(items: &mut Vec<String>) {
    let mut seen = std::collections::BTreeSet::new();
    items.retain(|item| seen.insert(item.clone()));
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".into();
    }
    let escaped = value.replace('\'', "'\"'\"'");
    format!("'{escaped}'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_pending_cleanup_and_live_resources() {
        let cleanup_hints = vec!["wait for integration".to_string()];
        let item = detect_cleanup_debt(&CleanupDebtInput {
            plan_id: "pl_1",
            task_id: "task_1",
            task_name: "Lane",
            task_status: "done",
            is_terminal: true,
            cleanup_status: None,
            cleanup_hints: &cleanup_hints,
            tab_id: Some("tab-1"),
            tab_live: true,
            worktree_path: Some("/tmp/wt"),
            worktree_exists: true,
            branch: Some("lane/x"),
            project_path: "/repo",
        })
        .unwrap();
        assert!(item
            .issues
            .iter()
            .any(|issue| issue.kind == CleanupDebtKind::PendingStatus));
        assert!(item
            .issues
            .iter()
            .any(|issue| issue.kind == CleanupDebtKind::LiveTab));
        assert!(item.commands.iter().any(|command| command
            .contains("codeterm plan task update 'pl_1' 'task_1' --cleanup-status done")));
        assert!(item
            .commands
            .iter()
            .any(|command| command.contains("git -C '/repo' worktree remove '/tmp/wt'")));
    }

    #[test]
    fn deferred_cleanup_still_surfaces_exact_commands() {
        let cleanup_hints = vec!["active dev Tab still needed".to_string()];
        let item = detect_cleanup_debt(&CleanupDebtInput {
            plan_id: "pl_1",
            task_id: "task_1",
            task_name: "Lane",
            task_status: "cancelled",
            is_terminal: true,
            cleanup_status: Some("deferred"),
            cleanup_hints: &cleanup_hints,
            tab_id: None,
            tab_live: false,
            worktree_path: None,
            worktree_exists: false,
            branch: None,
            project_path: "/repo",
        })
        .unwrap();
        assert!(item.deferred);
        assert!(render_cleanup_debt_hint(&item).contains("active dev Tab still needed"));
        assert_eq!(item.commands.len(), 1);
        assert_eq!(
            item.commands[0],
            "codeterm plan task update 'pl_1' 'task_1' --cleanup-status done"
        );
    }
}
