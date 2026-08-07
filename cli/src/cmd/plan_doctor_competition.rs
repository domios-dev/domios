use std::path::Path;

use serde_json::Value;

use crate::cmd::plan_doctor::Finding;

pub(super) fn push_competition_findings(
    task: &Value,
    plan_id: Option<String>,
    path: &Path,
    task_id: String,
    findings: &mut Vec<Finding>,
) {
    if competitive_live_attempts(task) != Some(1) {
        return;
    }
    findings.push(Finding {
        severity: "warning",
        kind: "competitive_single_attempt",
        plan_id,
        path: path.display().to_string(),
        task_id: Some(task_id),
        detail: "competitive task has only one live competition attempt".into(),
        fix: "spawn grouped attempts through the plan.task.compete route, or record the abandoned competitor".into(),
    });
}

fn competitive_live_attempts(task: &Value) -> Option<usize> {
    (task
        .get("competition")?
        .get("enabled")
        .and_then(Value::as_bool)
        == Some(true))
    .then(|| {
        task.get("execution_attempts")
            .and_then(Value::as_array)
            .map(|attempts| {
                attempts
                    .iter()
                    .filter(|attempt| {
                        attempt
                            .get("competition_group_id")
                            .and_then(Value::as_str)
                            .is_some()
                            && attempt.get("closed_at").is_none_or(Value::is_null)
                    })
                    .count()
            })
            .unwrap_or_default()
    })
}
