mod quality;
mod work_facet;

use std::path::Path;
use std::process::Command;

use super::*;

/// The doctor's only production path asks the daemon for the plan, so tests serve
/// a plan value over a stub client instead of storage files it never reads.
pub(super) struct StubPlanRpc {
    plan: Value,
    calls: Vec<String>,
}

impl crate::client::RpcClient for StubPlanRpc {
    fn call_detailed(
        &mut self,
        method: &str,
        _params: Option<Value>,
    ) -> Result<Value, crate::client::SocketCallError> {
        self.calls.push(method.to_string());
        match method {
            "plan.get" => Ok(self.plan.clone()),
            "plan.files" => Ok(json!({
                "index_path": "C:/central/plans/active/pl_1/INDEX.md",
                "entries": []
            })),
            other => panic!("unexpected RPC method {other}"),
        }
    }
}

pub(super) fn authoritative_report(
    plan: &Value,
    plan_id: &str,
    project: &Path,
    kind: &str,
) -> Value {
    authoritative_report_with_calls(plan, plan_id, project, kind).0
}

pub(super) fn authoritative_report_with_calls(
    plan: &Value,
    plan_id: &str,
    project: &Path,
    kind: &str,
) -> (Value, Vec<String>) {
    let mut client = StubPlanRpc {
        plan: plan.clone(),
        calls: Vec::new(),
    };
    let report = doctor_report_authoritative(
        &mut client,
        Some(plan_id.to_string()),
        Some(project.display().to_string()),
        kind,
    )
    .unwrap();
    (report, client.calls)
}

pub(super) fn run_git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

// Materialize a plan Value into the on-disk storage-v2 layout: plan.yaml +
// goal.yaml + one tasks/<id>.yaml per task, matching what the daemon writes.
pub(super) fn write_v2_plan(plan_dir: &Path, plan: &Value) {
    std::fs::create_dir_all(plan_dir).unwrap();
    let mut plan = plan.clone();
    let obj = plan.as_object_mut().unwrap();
    if let Some(goal) = obj.remove("goal") {
        std::fs::write(
            plan_dir.join("goal.yaml"),
            serde_yaml::to_string(&goal).unwrap(),
        )
        .unwrap();
    }
    if let Some(Value::Array(tasks)) = obj.remove("tasks") {
        if !tasks.is_empty() {
            let tasks_dir = plan_dir.join("tasks");
            std::fs::create_dir_all(&tasks_dir).unwrap();
            for task in &tasks {
                let id = task.get("id").and_then(Value::as_str).unwrap_or("task");
                std::fs::write(
                    tasks_dir.join(format!("{id}.yaml")),
                    serde_yaml::to_string(task).unwrap(),
                )
                .unwrap();
            }
        }
    }
    std::fs::write(
        plan_dir.join("plan.yaml"),
        serde_yaml::to_string(&plan).unwrap(),
    )
    .unwrap();
}

#[test]
fn detects_missing_milestone_from() {
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "sections": [],
        "tasks": [{
            "id": "task_1",
            "name": "Task",
            "status": "done",
            "milestones": [{"id": "ms_1", "kind": "status", "ts": 1}]
        }]
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "storage");
    assert!(findings
        .iter()
        .any(|f| f.kind == "missing_milestone_field" && f.detail.contains("'from'")));
}

#[test]
fn detects_missing_parent_and_section() {
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "sections": [{"id": "sec_1", "title": "Spec"}],
        "tasks": [{
            "id": "task_1",
            "name": "Task",
            "status": "pending",
            "parent_task_id": "missing",
            "section_ids": ["missing_sec"]
        }]
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "structure");
    assert!(findings.iter().any(|f| f.kind == "missing_parent_task"));
    assert!(findings.iter().any(|f| f.kind == "missing_section"));
}

#[test]
fn warns_when_completed_group_has_no_regression_and_silent_after_record() {
    let base = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "sections": [],
        "tasks": [
            {"id": "task_1", "name": "A", "status": "done", "group": "phase-1", "status_changed_at": 10},
            {"id": "task_2", "name": "B", "status": "done", "group": "phase-1", "status_changed_at": 20}
        ]
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &base, "structure");
    assert!(findings.iter().any(|f| f.kind == "group_missing_regression"
        && f.severity == "warning"
        && f.detail.contains("phase-1")));

    // A regression stride recorded after the last close silences the warning.
    let mut recorded = base.clone();
    recorded["strides"] = json!([
        {"id": "ms_r", "kind": "regression", "ts": 30, "links": ["group:phase-1"]}
    ]);
    let findings = inspect_plan(Path::new("pl_1.json"), &recorded, "structure");
    assert!(findings
        .iter()
        .all(|f| f.kind != "group_missing_regression"));

    // A stale record (predating a later re-close) re-arms the warning.
    let mut stale = base.clone();
    stale["tasks"][1]["status_changed_at"] = json!(50);
    stale["strides"] = json!([
        {"id": "ms_r", "kind": "regression", "ts": 30, "links": ["group:phase-1"]}
    ]);
    let findings = inspect_plan(Path::new("pl_1.json"), &stale, "structure");
    assert!(findings
        .iter()
        .any(|f| f.kind == "group_missing_regression"));
}

#[test]
fn warns_when_completed_group_has_no_retrospective_and_rearms_on_reclose() {
    let mut plan = json!({
        "id": "pl_1", "title": "Plan", "status": "active", "sections": [],
        "tasks": [
            {"id": "t1", "name": "A", "status": "done", "group": "phase-1", "status_changed_at": 10},
            {"id": "t2", "name": "B", "status": "done", "group": "phase-1", "status_changed_at": 20}
        ],
        "strides": [{"id": "ms_r", "kind": "regression", "ts": 30, "links": ["group:phase-1"]}]
    });
    let flagged = |plan: &Value| {
        inspect_plan(Path::new("pl_1.json"), plan, "structure")
            .iter()
            .any(|f| f.kind == "group_missing_retrospective" && f.detail.contains("phase-1"))
    };
    assert!(flagged(&plan));

    plan["strides"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "ms_c", "kind": "group_close", "ts": 40, "links": ["group:phase-1"]}));
    assert!(!flagged(&plan));

    plan["tasks"][1]["status_changed_at"] = json!(50);
    assert!(flagged(&plan));
}

#[test]
fn incomplete_or_all_cancelled_group_is_not_flagged() {
    let incomplete = json!({
        "id": "pl_1", "title": "Plan", "status": "active", "sections": [],
        "tasks": [
            {"id": "t1", "name": "A", "status": "done", "group": "g", "status_changed_at": 10},
            {"id": "t2", "name": "B", "status": "pending", "group": "g", "status_changed_at": 5}
        ]
    });
    assert!(
        inspect_plan(Path::new("pl_1.json"), &incomplete, "structure")
            .iter()
            .all(|f| f.kind != "group_missing_regression")
    );

    let all_cancelled = json!({
        "id": "pl_1", "title": "Plan", "status": "active", "sections": [],
        "tasks": [
            {"id": "t1", "name": "A", "status": "cancelled", "group": "g", "status_changed_at": 10},
            {"id": "t2", "name": "B", "status": "cancelled", "group": "g", "status_changed_at": 20}
        ]
    });
    assert!(
        inspect_plan(Path::new("pl_1.json"), &all_cancelled, "structure")
            .iter()
            .all(|f| f.kind != "group_missing_regression")
    );
}

#[test]
fn detects_malformed_json() {
    let dir = tempfile::tempdir().unwrap();
    // The authoritative path never reads files, so a malformed plan reaches the
    // doctor as a non-object value from plan.get.
    let report = authoritative_report(&json!("{not yaml"), "pl_bad", dir.path(), "all");
    assert!(!report["ok"].as_bool().unwrap());
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["kind"] == "malformed_plan"));
}

#[test]
fn storage_warns_about_stray_linked_worktree_sidecar_copy() {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("main");
    let linked = tmp.path().join("linked");
    std::fs::create_dir(&main).unwrap();
    run_git(&main, &["init"]);
    run_git(&main, &["config", "user.email", "test@example.com"]);
    run_git(&main, &["config", "user.name", "Test User"]);
    std::fs::write(main.join("README.md"), "root\n").unwrap();
    run_git(&main, &["add", "README.md"]);
    run_git(&main, &["commit", "-m", "init"]);
    run_git(
        &main,
        &[
            "worktree",
            "add",
            linked.to_str().unwrap(),
            "-b",
            "linked-test",
        ],
    );

    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "owner": {},
        "project_path": main.display().to_string(),
        "canonical_project_path": main.display().to_string(),
        "created_at": 1,
        "updated_at": 2,
        "tasks": []
    });
    let canonical_dir = main.join(".codeterm/plans/active/pl_1");
    let stray_dir = linked.join(".codeterm/plans/active/pl_1");
    write_v2_plan(&canonical_dir, &plan);
    write_v2_plan(&stray_dir, &plan);

    let report = authoritative_report(&plan, "pl_1", &linked, "storage");
    let findings = report["findings"].as_array().unwrap();
    // The comparison uses production canonicalization because temporary paths differ across Windows and Unix.
    let main_c = super::inspect::canonicalize_path(&main)
        .display()
        .to_string();
    let linked_c = super::inspect::canonicalize_path(&linked)
        .display()
        .to_string();
    assert!(findings.iter().any(|f| {
        f["kind"] == "stray_sidecar_copy"
            && f["detail"].as_str().unwrap().contains(&main_c)
            && f["path"].as_str().unwrap().contains(&linked_c)
    }));
}

#[test]
fn detects_duplicate_handle_and_parent_cycle() {
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "sections": [],
        "tasks": [
            {
                "id": "task_a",
                "name": "A",
                "status": "pending",
                "handle": "dup",
                "parent_task_id": "task_b"
            },
            {
                "id": "task_b",
                "name": "B",
                "status": "pending",
                "handle": "dup",
                "parent_task_id": "task_a"
            }
        ]
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "structure");
    assert!(findings.iter().any(|f| f.kind == "duplicate_task_handle"));
    assert!(findings.iter().any(|f| f.kind == "parent_cycle"));
}

#[test]
fn renders_summary() {
    let report = json!({
        "checked_plans": 1,
        "error_count": 1,
        "warning_count": 0,
        "findings": [{
            "severity": "error",
            "kind": "missing_parent_task",
            "plan_id": "pl_1",
            "task_id": "task_1",
            "detail": "parent missing",
            "fix": "clear parent"
        }]
    });
    let rendered = render_doctor(&report);
    assert!(rendered.contains("Plan doctor: 1 checked, 1 errors"));
    assert!(rendered.contains("missing_parent_task"));
}
