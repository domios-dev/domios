use std::path::Path;

use serde_json::{json, Value};

use super::Finding;
use crate::client::RpcClient;

const IN_PROGRESS_TAB_GONE: &str = "in_progress_tab_gone";

/// Tab liveness lives in the daemon, so the doctor reads it from reconcile
/// (without fixing anything) instead of guessing from the stored plan.
pub(super) fn liveness_findings(
    client: &mut dyn RpcClient,
    plan_id: &str,
    path: &Path,
) -> Vec<Finding> {
    let report = match client.call_detailed(
        "plan.reconcile",
        Some(json!({ "plan_id": plan_id, "fix": false })),
    ) {
        Ok(report) => report,
        Err(error) => {
            return vec![Finding {
                severity: "warning",
                kind: "liveness_unchecked",
                plan_id: Some(plan_id.to_string()),
                path: path.display().to_string(),
                task_id: None,
                detail: format!("could not read tab liveness from plan.reconcile: {error}"),
                fix: format!("rerun `codeterm plan reconcile {plan_id}` once the daemon answers"),
            }];
        }
    };
    report
        .get("drift")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("kind").and_then(Value::as_str) == Some(IN_PROGRESS_TAB_GONE))
        .map(|item| {
            let task_id = item.get("task_id").and_then(Value::as_str).unwrap_or("?");
            Finding {
                severity: "error",
                kind: IN_PROGRESS_TAB_GONE,
                plan_id: Some(plan_id.to_string()),
                path: path.display().to_string(),
                task_id: Some(task_id.to_string()),
                detail: item
                    .get("detail")
                    .and_then(Value::as_str)
                    .unwrap_or("task is in_progress but its Worker tab is gone")
                    .to_string(),
                fix: format!(
                    "recover the lost Worker: `codeterm plan task respawn {plan_id} {task_id} --attempt <attempt>` \
                     (attempt ids: `codeterm plan task attempts {plan_id} {task_id}`), or `codeterm plan reconcile {plan_id} --fix`"
                ),
            }
        })
        .collect()
}
