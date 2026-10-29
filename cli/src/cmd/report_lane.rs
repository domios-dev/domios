use std::path::PathBuf;

use serde_json::Value;

use crate::client::RpcClient;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskReadinessScope {
    pub worktree_path: Option<PathBuf>,
    pub recorded_checkout: bool,
    pub canonical_writer: bool,
    pub canonical_checkout_path: Option<PathBuf>,
    pub base: Option<String>,
    pub requires_repository: bool,
}

pub fn task_readiness_scope(
    client: &mut dyn RpcClient,
    plan_id: Option<&str>,
    task_id: Option<&str>,
    tab_id: &str,
) -> Result<Option<TaskReadinessScope>, crate::cmd::report_guard::ReportRefusal> {
    let Some(task_id) = task_id else {
        return Ok(None);
    };
    let unavailable = |detail: &str| crate::cmd::report_guard::unavailable_scope_refusal(detail);
    let plan_id = plan_id.ok_or_else(|| unavailable("task has no plan binding"))?;
    let plan = crate::cmd::plan::call_plan_rpc(
        client,
        "plan.get",
        serde_json::json!({ "plan_id": plan_id }),
    )
    .map_err(|_| unavailable("plan lookup failed"))?;
    task_readiness_scope_from_plan(&plan, task_id, tab_id)
        .map(Some)
        .ok_or_else(|| unavailable("task lookup was missing or malformed"))
}

fn task_readiness_scope_from_plan(
    plan: &Value,
    task_id: &str,
    tab_id: &str,
) -> Option<TaskReadinessScope> {
    let task = find_task(plan, task_id)?;
    if task.get("execution_intent").is_some_and(|intent| {
        !matches!(
            intent.as_str(),
            Some("unspecified" | "read_only" | "mutating" | "non_repository")
        )
    }) || task
        .get("lane")
        .is_some_and(|lane| !lane.is_null() && !lane.is_object())
        || task
            .get("execution_attempts")
            .is_some_and(|attempts| !attempts.is_array())
    {
        return None;
    }
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
    let checkout = attempt.and_then(|attempt| nonempty(attempt.get("checkout_path")));
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
        worktree_path: checkout
            .clone()
            .or_else(|| record.and_then(|record| nonempty(record.get("worktree_path"))))
            .map(|path| PathBuf::from(codeterm_paths::strip_verbatim_prefix(&path))),
        recorded_checkout: checkout.is_some(),
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
            recorded_checkout: false,
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
                && attempt.get("retired_at").is_none_or(Value::is_null)
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
mod tests;
