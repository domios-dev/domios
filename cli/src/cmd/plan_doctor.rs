use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::client::{RpcClient, SocketCallError};
use crate::cmd::plan_local::{plan_id_from, project_root};

#[cfg(test)]
mod authoritative_tests;
mod cleanup;
mod inspect;
mod quality;

use inspect::inspect_plan;
use inspect::inspect_stray_sidecars;

// Mirrors src-tauri/src/plan/reconcile.rs PENDING_UNSTAFFED_STALE_MS.
pub(super) const PENDING_UNSTAFFED_STALE_MS: u64 = 4 * 60 * 60 * 1000;
pub(super) const STALLED_LOOP_AFTER_MS: u64 = 3 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone)]
pub(super) struct Finding {
    pub(super) severity: &'static str,
    pub(super) kind: &'static str,
    pub(super) plan_id: Option<String>,
    pub(super) path: String,
    pub(super) task_id: Option<String>,
    pub(super) detail: String,
    pub(super) fix: String,
}

impl Finding {
    fn json(&self) -> Value {
        json!({
            "severity": self.severity,
            "kind": self.kind,
            "plan_id": self.plan_id,
            "path": self.path,
            "task_id": self.task_id,
            "detail": self.detail,
            "fix": self.fix,
        })
    }
}

pub fn run(
    client: &mut dyn RpcClient,
    json_mode: bool,
    plan_id: Option<String>,
    project: Option<String>,
    kind: String,
) -> Result<Option<Value>, String> {
    let report = doctor_report_authoritative(client, plan_id, project, &kind)?;
    let ok = report.get("ok").and_then(Value::as_bool).unwrap_or(false);
    if json_mode {
        if !ok {
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
            std::process::exit(1);
        }
        return Ok(Some(report));
    }
    print!("{}", render_doctor(&report));
    if !ok {
        std::process::exit(1);
    }
    Ok(None)
}

pub(crate) fn assert_quality_ready(
    client: &mut dyn RpcClient,
    plan_id: &str,
) -> Result<(), String> {
    let mut report =
        doctor_report_authoritative(client, Some(plan_id.to_string()), None, "quality")?;
    remove_approval_transition_findings(&mut report);
    escalate_approval_blocking_warnings(&mut report);
    if report.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return Ok(());
    }
    Err(render_quality_approval_error(plan_id, &report))
}

/// Warnings the standalone doctor only reports but approval refuses to pass.
/// `missing_executor_role` is here because approval is where the creation paths
/// converge: HTTP and socket callers never see the `plan task add` requirement.
const APPROVAL_BLOCKING_WARNINGS: [&str; 2] = ["planner_unpaired", "missing_executor_role"];

fn recount_report(report: &mut Value) {
    let findings = report
        .get("findings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let count = |want: &str| {
        findings
            .iter()
            .filter(|f| f.get("severity").and_then(Value::as_str) == Some(want))
            .count()
    };
    let error_count = count("error");
    let warning_count = count("warning");
    if let Some(object) = report.as_object_mut() {
        object.insert("error_count".into(), json!(error_count));
        object.insert("warning_count".into(), json!(warning_count));
        object.insert("ok".into(), json!(error_count == 0));
    }
}

fn remove_approval_transition_findings(report: &mut Value) {
    let Some(findings) = report.get_mut("findings").and_then(Value::as_array_mut) else {
        return;
    };
    findings.retain(|finding| {
        finding.get("kind").and_then(Value::as_str) != Some("tasks_before_approval")
    });
    recount_report(report);
}

fn escalate_approval_blocking_warnings(report: &mut Value) {
    let Some(findings) = report.get_mut("findings").and_then(Value::as_array_mut) else {
        return;
    };
    let mut escalated = false;
    for finding in findings.iter_mut() {
        let kind = finding.get("kind").and_then(Value::as_str).unwrap_or("");
        if APPROVAL_BLOCKING_WARNINGS.contains(&kind)
            && finding.get("severity").and_then(Value::as_str) == Some("warning")
        {
            finding["severity"] = json!("error");
            escalated = true;
        }
    }
    if escalated {
        recount_report(report);
    }
}

fn render_quality_approval_error(plan_id: &str, report: &Value) -> String {
    format!(
        "✗ plan approval blocked by quality doctor\n\n{}fix:\n  codeterm plan doctor {plan_id} --kind quality\n  codeterm plan approve {plan_id}",
        render_doctor(report)
    )
}

fn doctor_report_authoritative(
    client: &mut dyn RpcClient,
    plan_id: Option<String>,
    project: Option<String>,
    kind: &str,
) -> Result<Value, String> {
    let project_root = project_root(project)?;
    let requested_id = plan_id.as_deref();
    let ids = if let Some(id) = requested_id {
        vec![id.to_string()]
    } else {
        authoritative_plan_ids(client, &project_root)?
    };
    let mut findings = Vec::new();
    let mut checked = 0usize;
    for id in ids {
        let params = json!({ "plan_id": id });
        let plan = match client.call_detailed("plan.get", Some(params)) {
            Ok(plan) => plan,
            Err(SocketCallError::Rpc(error))
                if error
                    .data
                    .as_ref()
                    .and_then(|data| data.get("type"))
                    .and_then(Value::as_str)
                    == Some("not_found") =>
            {
                continue;
            }
            Err(error) => {
                findings.push(malformed_plan_finding(
                    &id,
                    authoritative_plan_path(client, &id, &project_root),
                    error.to_string(),
                ));
                continue;
            }
        };
        checked += 1;
        let path = authoritative_plan_path(client, &id, &project_root);
        if !plan.is_object() {
            findings.push(malformed_plan_finding(
                &id,
                path,
                "plan.get returned a non-object plan value".into(),
            ));
            continue;
        }
        findings.extend(inspect_plan(&path, &plan, kind));
        if matches!(kind, "all" | "storage") {
            findings.extend(inspect_stray_sidecars(&project_root, &path, &plan));
        }
    }
    if checked == 0 {
        if let Some(id) = requested_id {
            findings.push(Finding {
                severity: "error",
                kind: "plan_not_inspected",
                plan_id: Some(id.to_string()),
                path: project_root.join(id).display().to_string(),
                task_id: None,
                detail: format!("explicit plan '{id}' was not found in the authoritative store"),
                fix: "verify the plan id with `codeterm plan list --all`, then rerun plan doctor"
                    .into(),
            });
        }
    }
    Ok(report_value(project_root, kind, checked, findings))
}

fn authoritative_plan_ids(
    client: &mut dyn RpcClient,
    project_root: &Path,
) -> Result<Vec<String>, String> {
    let response = client
        .call_detailed(
            "plan.list",
            Some(json!({ "project": project_root.display().to_string() })),
        )
        .map_err(|error| error.to_string())?;
    let plans = response
        .get("plans")
        .and_then(Value::as_array)
        .ok_or_else(|| "plan.list returned no plans array".to_string())?;
    Ok(plans
        .iter()
        .filter_map(|plan| plan.get("plan_id").and_then(Value::as_str))
        .map(str::to_string)
        .collect())
}

fn authoritative_plan_path(
    client: &mut dyn RpcClient,
    plan_id: &str,
    project_root: &Path,
) -> PathBuf {
    client
        .call_detailed("plan.files", Some(json!({ "plan_id": plan_id })))
        .ok()
        .and_then(|manifest| {
            manifest
                .get("index_path")
                .and_then(Value::as_str)
                .map(PathBuf::from)
        })
        .and_then(|path| path.parent().map(|dir| dir.join("plan.yaml")))
        .unwrap_or_else(|| project_root.join(format!("{plan_id}.json")))
}

fn malformed_plan_finding(plan_id: &str, path: PathBuf, detail: String) -> Finding {
    Finding {
        severity: "error",
        kind: "malformed_plan",
        plan_id: Some(plan_id.to_string()),
        path: path.display().to_string(),
        task_id: None,
        detail,
        fix: "repair the authoritative plan record, then rerun plan doctor".into(),
    }
}

fn report_value(
    project_root: PathBuf,
    kind: &str,
    checked: usize,
    findings: Vec<Finding>,
) -> Value {
    let error_count = findings
        .iter()
        .filter(|finding| finding.severity == "error")
        .count();
    json!({
        "ok": error_count == 0,
        "project": project_root.display().to_string(),
        "kind": kind,
        "checked_plans": checked,
        "error_count": error_count,
        "warning_count": findings.iter().filter(|finding| finding.severity == "warning").count(),
        "findings": findings.iter().map(Finding::json).collect::<Vec<_>>(),
    })
}

pub(super) fn plan_id(plan: &Value, path: &Path) -> Option<String> {
    plan_id_from(plan, path)
}

pub(crate) fn render_doctor(report: &Value) -> String {
    let findings = report
        .get("findings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out = format!(
        "Plan doctor: {} checked, {} errors, {} warnings\n",
        report
            .get("checked_plans")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        report
            .get("error_count")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        report
            .get("warning_count")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    );
    for finding in findings {
        let severity = finding
            .get("severity")
            .and_then(Value::as_str)
            .unwrap_or("?");
        let kind = finding.get("kind").and_then(Value::as_str).unwrap_or("?");
        let plan = finding
            .get("plan_id")
            .and_then(Value::as_str)
            .unwrap_or("?");
        let task = finding.get("task_id").and_then(Value::as_str).unwrap_or("");
        let detail = finding.get("detail").and_then(Value::as_str).unwrap_or("");
        let fix = finding.get("fix").and_then(Value::as_str).unwrap_or("");
        let task_suffix = if task.is_empty() {
            "".into()
        } else {
            format!(" task={task}")
        };
        out.push_str(&format!(
            "  - {severity} {kind} plan={plan}{task_suffix}: {detail}; fix: {fix}\n"
        ));
    }
    out
}

#[cfg(test)]
mod approval_transition_tests;
#[cfg(test)]
mod tests;
