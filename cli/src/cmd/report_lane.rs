use std::path::PathBuf;

use serde_json::Value;

use crate::client::SocketClient;

/// The worktree a lane owns, resolved from plan state rather than the pane cwd.
/// Precedence mirrors `CODETERM_LANE_BASE`: this tab's open attempt, then the lane.
pub fn lane_worktree(
    client: &mut SocketClient,
    plan_id: Option<&str>,
    task_id: Option<&str>,
    tab_id: &str,
) -> Option<PathBuf> {
    let plan_id = plan_id?;
    let task_id = task_id?;
    let plan = crate::cmd::plan::call_plan_rpc(
        client,
        "plan.get",
        serde_json::json!({ "plan_id": plan_id }),
    )
    .ok()?;
    let task = find_task(&plan, task_id)?;
    lane_worktree_from_task(task, tab_id).map(PathBuf::from)
}

fn find_task<'a>(plan: &'a Value, task_id: &str) -> Option<&'a Value> {
    plan.get("tasks")?.as_array()?.iter().find(|task| {
        matches(task.get("id"), task_id) || matches(task.get("handle"), task_id)
    })
}

fn matches(value: Option<&Value>, expected: &str) -> bool {
    value.and_then(Value::as_str) == Some(expected)
}

fn lane_worktree_from_task(task: &Value, tab_id: &str) -> Option<String> {
    open_attempt_worktree(task, tab_id).or_else(|| {
        nonempty(
            task.get("lane")
                .and_then(|lane| lane.get("worktree_path")),
        )
    })
}

fn open_attempt_worktree(task: &Value, tab_id: &str) -> Option<String> {
    task.get("execution_attempts")?
        .as_array()?
        .iter()
        .rev()
        .find(|attempt| {
            matches(attempt.get("executor_tab_id"), tab_id)
                && attempt.get("closed_at").is_none_or(Value::is_null)
        })
        .and_then(|attempt| nonempty(attempt.get("worktree_path")))
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
    use serde_json::json;

    #[test]
    fn falls_back_to_the_task_lane_worktree() {
        let task = json!({
            "id": "task_a",
            "lane": { "worktree_path": "/lanes/a" },
        });

        assert_eq!(
            lane_worktree_from_task(&task, "tab-a").as_deref(),
            Some("/lanes/a")
        );
    }

    #[test]
    fn open_attempt_for_this_tab_wins_over_the_lane_record() {
        let task = json!({
            "id": "task_a",
            "lane": { "worktree_path": "/lanes/stale" },
            "execution_attempts": [
                { "executor_tab_id": "tab-a", "worktree_path": "/lanes/live" },
            ],
        });

        assert_eq!(
            lane_worktree_from_task(&task, "tab-a").as_deref(),
            Some("/lanes/live")
        );
    }

    #[test]
    fn closed_and_foreign_attempts_are_ignored() {
        let task = json!({
            "id": "task_a",
            "lane": { "worktree_path": "/lanes/a" },
            "execution_attempts": [
                { "executor_tab_id": "tab-a", "worktree_path": "/lanes/closed", "closed_at": "t1" },
                { "executor_tab_id": "tab-b", "worktree_path": "/lanes/other" },
            ],
        });

        assert_eq!(
            lane_worktree_from_task(&task, "tab-a").as_deref(),
            Some("/lanes/a")
        );
    }

    #[test]
    fn absent_lane_worktree_resolves_to_none() {
        let task = json!({ "id": "task_a", "lane": { "worktree_path": "  " } });

        assert_eq!(lane_worktree_from_task(&task, "tab-a"), None);
    }

    #[test]
    fn task_is_found_by_handle() {
        let plan = json!({ "tasks": [{ "id": "task_a", "handle": "alpha" }] });

        assert_eq!(find_task(&plan, "alpha"), Some(&plan["tasks"][0]));
        assert_eq!(find_task(&plan, "missing"), None);
    }
}
