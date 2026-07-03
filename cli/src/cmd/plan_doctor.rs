use std::path::Path;

use serde_json::{json, Value};

use crate::cmd::plan_local::{plan_files, plan_id_from, project_root};

mod cleanup;
mod inspect;
mod quality;

use inspect::inspect_plan;
use inspect::inspect_stray_sidecars;
use quality::inspect_task_lifecycle;

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
    json_mode: bool,
    plan_id: Option<String>,
    project: Option<String>,
    kind: String,
) -> Result<Option<Value>, String> {
    let report = doctor_report(plan_id, project, &kind)?;
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

pub(crate) fn assert_quality_ready(plan_id: &str) -> Result<(), String> {
    let mut report = doctor_report(Some(plan_id.to_string()), None, "quality")?;
    escalate_planner_unpaired_for_approval(&mut report);
    if report.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return Ok(());
    }
    Err(render_quality_approval_error(plan_id, &report))
}

// task_aa01c0db: an orchestrator without a bound/waived planner peer is only
// a WARNING day-to-day (`codeterm plan doctor`), but approval is the one
// hard gate — escalate it to an ERROR here rather than in `inspect_quality`
// so the ladder doesn't leak into every other `inspect_plan` call site.
fn escalate_planner_unpaired_for_approval(report: &mut Value) {
    let Some(findings) = report.get_mut("findings").and_then(Value::as_array_mut) else {
        return;
    };
    let mut escalated = false;
    for finding in findings.iter_mut() {
        if finding.get("kind").and_then(Value::as_str) == Some("planner_unpaired")
            && finding.get("severity").and_then(Value::as_str) == Some("warning")
        {
            finding["severity"] = json!("error");
            escalated = true;
        }
    }
    if !escalated {
        return;
    }
    let findings = report
        .get("findings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let error_count = findings
        .iter()
        .filter(|f| f.get("severity").and_then(Value::as_str) == Some("error"))
        .count();
    let warning_count = findings
        .iter()
        .filter(|f| f.get("severity").and_then(Value::as_str) == Some("warning"))
        .count();
    if let Some(obj) = report.as_object_mut() {
        obj.insert("error_count".into(), json!(error_count));
        obj.insert("warning_count".into(), json!(warning_count));
        obj.insert("ok".into(), json!(error_count == 0));
    }
}

fn render_quality_approval_error(plan_id: &str, report: &Value) -> String {
    format!(
        "✗ plan approval blocked by quality doctor\n\n{}fix:\n  codeterm plan doctor {plan_id} --kind quality\n  codeterm plan approve {plan_id}",
        render_doctor(&report)
    )
}

fn doctor_report(
    plan_id: Option<String>,
    project: Option<String>,
    kind: &str,
) -> Result<Value, String> {
    let project_root = project_root(project)?;
    let plans_dir = project_root.join(".codeterm").join("plans");
    let files = plan_files(&plans_dir, plan_id.as_deref())?;
    let mut findings = Vec::new();
    let mut checked = 0usize;
    for path in files {
        checked += 1;
        match std::fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str::<Value>(&raw) {
                Ok(plan) => {
                    findings.extend(inspect_plan(&path, &plan, kind));
                    if matches!(kind, "all" | "storage") {
                        findings.extend(inspect_stray_sidecars(&project_root, &path, &plan));
                    }
                }
                Err(err) => findings.push(Finding {
                    severity: "error",
                    kind: "malformed_json",
                    plan_id: path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map(str::to_string),
                    path: path.display().to_string(),
                    task_id: None,
                    detail: err.to_string(),
                    fix: "open the JSON file, repair syntax, then rerun plan doctor".into(),
                }),
            },
            Err(err) => findings.push(Finding {
                severity: "error",
                kind: "unreadable_file",
                plan_id: path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_string),
                path: path.display().to_string(),
                task_id: None,
                detail: err.to_string(),
                fix: "check file permissions or remove the stale plan entry".into(),
            }),
        }
    }
    let error_count = findings.iter().filter(|f| f.severity == "error").count();
    Ok(json!({
        "ok": error_count == 0,
        "project": project_root.display().to_string(),
        "kind": kind,
        "checked_plans": checked,
        "error_count": error_count,
        "warning_count": findings.iter().filter(|f| f.severity == "warning").count(),
        "findings": findings.iter().map(Finding::json).collect::<Vec<_>>(),
    }))
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
mod tests;
