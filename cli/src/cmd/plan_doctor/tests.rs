use super::*;

fn run_git(cwd: &Path, args: &[&str]) {
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
fn detects_malformed_json() {
    let dir = tempfile::tempdir().unwrap();
    let plan_dir = dir.path().join(".codeterm/plans/active/pl_bad");
    std::fs::create_dir_all(&plan_dir).unwrap();
    std::fs::write(plan_dir.join("plan.json"), "{not json").unwrap();
    let report = doctor_report(None, Some(dir.path().display().to_string()), "all").unwrap();
    assert!(!report["ok"].as_bool().unwrap());
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["kind"] == "malformed_json"));
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
    std::fs::create_dir_all(&canonical_dir).unwrap();
    std::fs::create_dir_all(&stray_dir).unwrap();
    let raw = serde_json::to_string_pretty(&plan).unwrap();
    std::fs::write(canonical_dir.join("plan.json"), &raw).unwrap();
    std::fs::write(stray_dir.join("plan.json"), raw).unwrap();

    let report = doctor_report(None, Some(linked.display().to_string()), "storage").unwrap();
    let findings = report["findings"].as_array().unwrap();
    assert!(findings.iter().any(|f| {
        f["kind"] == "stray_sidecar_copy"
            && f["detail"]
                .as_str()
                .unwrap()
                .contains(main.to_str().unwrap())
            && f["path"]
                .as_str()
                .unwrap()
                .contains(linked.to_str().unwrap())
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

#[test]
fn approval_error_includes_doctor_recovery_commands() {
    let report = json!({
        "checked_plans": 1,
        "error_count": 1,
        "warning_count": 0,
        "findings": [{
            "severity": "error",
            "kind": "missing_goal",
            "plan_id": "pl_1",
            "task_id": null,
            "detail": "plan has no goal vector",
            "fix": "set the north-star first"
        }]
    });
    let rendered = render_quality_approval_error("pl_1", &report);
    assert!(rendered.contains("plan approval blocked by quality doctor"));
    assert!(rendered.contains("codeterm plan doctor pl_1 --kind quality"));
    assert!(rendered.contains("codeterm plan approve pl_1"));
}

#[test]
fn quality_blocks_unapproved_or_underplanned_work() {
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "sections": [{
            "id": "sec_1",
            "title": "Requirements",
            "body": "WHAT/WHY: evidence and accepted gap"
        }],
        "tasks": [],
        "heartbeat": {"current_focus": "remember policy"}
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    let kinds = findings.iter().map(|f| f.kind).collect::<Vec<_>>();
    assert!(kinds.contains(&"missing_goal"));
    assert!(kinds.contains(&"missing_spec_source"));
    assert!(kinds.contains(&"heartbeat_without_execution"));
    assert!(kinds.contains(&"planning_mode_no_tasks"));
    assert!(findings
        .iter()
        .any(|f| f.kind == "missing_goal" && f.severity == "error"));
    assert!(findings
        .iter()
        .any(|f| f.kind == "planning_mode_no_tasks" && f.severity == "warning"));
}

#[test]
fn planning_kind_tasks_do_not_trip_approval_gate() {
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "mode": "planning",
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_1",
        "sections": [{"id": "sec_1", "title": "Spec", "body": "WHAT/WHY"}],
        "tasks": [{
            "id": "task_1",
            "name": "Improve task graph",
            "kind": "planning",
            "status": "pending",
            "section_ids": ["sec_1"]
        }]
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    assert!(
        !findings.iter().any(|f| f.kind == "tasks_before_approval"),
        "planning-kind tasks must not trip the approval gate"
    );
}

#[test]
fn execution_kind_task_in_planning_mode_trips_approval_gate() {
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "mode": "planning",
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_1",
        "sections": [{"id": "sec_1", "title": "Spec", "body": "WHAT/WHY"}],
        "tasks": [{
            "id": "task_1",
            "name": "Build feature",
            "kind": "implement",
            "status": "pending",
            "section_ids": ["sec_1"]
        }]
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    assert!(
        findings.iter().any(|f| f.kind == "tasks_before_approval"),
        "execution-kind tasks in planning mode must trip the approval gate"
    );
}

#[test]
fn quality_warns_on_terminal_task_cleanup_debt() {
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "mode": "execution",
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_1",
        "sections": [{"id": "sec_1", "title": "Spec", "body": "WHY"}],
        "tasks": [{
            "id": "task_1",
            "name": "Cleanup lane",
            "kind": "implement",
            "status": "done",
            "section_ids": ["sec_1"],
            "acceptance_criteria": ["done"],
            "test_plan": "n/a",
            "correctness": "n/a",
            "lane": {
                "worktree_path": "/definitely/missing/worktree",
                "branch": "lane/cleanup"
            }
        }]
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    assert!(findings.iter().any(|f| f.kind == "cleanup_debt"));
    let finding = findings.iter().find(|f| f.kind == "cleanup_debt").unwrap();
    assert!(finding.fix.contains("codeterm plan task update"));
    assert!(finding.fix.contains("--cleanup-status deferred"));
}

#[test]
fn quality_warns_on_deferred_cleanup() {
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "mode": "execution",
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_1",
        "sections": [{"id": "sec_1", "title": "Spec", "body": "WHY"}],
        "tasks": [{
            "id": "task_1",
            "name": "Cleanup lane",
            "kind": "implement",
            "status": "cancelled",
            "section_ids": ["sec_1"],
            "acceptance_criteria": ["done"],
            "test_plan": "n/a",
            "correctness": "n/a",
            "lane": {
                "cleanup_status": "deferred",
                "cleanup_hints": ["waiting for shared dev pane shutdown"]
            }
        }]
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    let finding = findings
        .iter()
        .find(|f| f.kind == "cleanup_deferred")
        .unwrap();
    assert!(finding.detail.contains("explicitly deferred"));
    assert!(finding.fix.contains("--cleanup-status done"));
}

#[test]
fn quality_errors_for_underspecified_tasks() {
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "mode": "approved",
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_1",
        "sections": [{
            "id": "sec_1",
            "title": "File ownership and approval gate",
            "body": "Exact paths, verification with build.sh check."
        }],
        "tasks": [{
            "id": "task_1",
            "name": "Do work",
            "status": "pending",
            "section_ids": []
        }]
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    assert!(findings.iter().any(|f| f.kind == "task_quality_gap"
        && f.task_id.as_deref() == Some("task_1")
        && f.severity == "error"
        && f.detail.contains("acceptance criteria")));
}

#[test]
fn quality_errors_for_parked_without_reason() {
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "mode": "approved",
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_1",
        "sections": [{"id": "sec_1", "title": "Execution", "body": "paths, build.sh check"}],
        "tasks": [{
            "id": "task_1",
            "name": "Do work",
            "status": "parked",
            "section_ids": ["sec_1"],
            "body": "b",
            "acceptance_criteria": ["c"],
            "test_plan": "t",
            "correctness": "x",
            "executor_role": "rust-backend-dev"
        }]
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    assert!(findings.iter().any(|f| f.kind == "parked_without_reason"
        && f.task_id.as_deref() == Some("task_1")
        && f.severity == "error"));
}

#[test]
fn quality_warns_for_aged_unstaffed_pending_task() {
    let old = 1_000u64;
    let now = old + PENDING_UNSTAFFED_STALE_MS + 1;
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "mode": "approved",
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_1",
        "updated_at": now,
        "sections": [{"id": "sec_1", "title": "Execution", "body": "paths, build.sh check"}],
        "tasks": [{
            "id": "task_1",
            "name": "Do work",
            "status": "pending",
            "status_changed_at": old,
            "section_ids": ["sec_1"],
            "body": "b",
            "acceptance_criteria": ["c"],
            "test_plan": "t",
            "correctness": "x",
            "executor_role": "rust-backend-dev"
        }]
    });
    let mut findings = Vec::new();
    inspect_task_lifecycle(
        Path::new("pl_1.json"),
        &plan,
        &plan["tasks"][0],
        now,
        &mut findings,
    );
    assert!(findings.iter().any(|f| f.kind == "aged_unstaffed_task"
        && f.task_id.as_deref() == Some("task_1")
        && f.severity == "warning"));
}

#[test]
fn quality_accepts_complete_task_and_plan_scaffold() {
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "mode": "approved",
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_1",
        "sections": [{
        "id": "sec_1",
        "title": "Execution",
        "body": "File ownership map with exact paths. Approval gate. Verification map: build.sh check. Wave sequencing: discovery is intermediate, then synthesis creates remaining tasks."
    }],
        "tasks": [{
            "id": "task_1",
            "name": "Do work",
            "status": "pending",
            "section_ids": ["sec_1"],
            "body": "Implement the scoped change.",
            "acceptance_criteria": ["criterion"],
            "test_plan": "run focused test",
            "correctness": "matches spec",
            "executor_role": "rust-backend-dev"
        }]
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn quality_warns_for_stalled_running_loop() {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "mode": "approved",
        "updated_at": now_ms.saturating_sub(STALLED_LOOP_AFTER_MS + 1),
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_1",
        "sections": [{"id": "sec_1", "title": "Execution", "body": "section"}],
        "tasks": [{
            "id": "task_1",
            "name": "Do work",
            "type": "implement",
            "status": "pending",
            "section_ids": ["sec_1"],
            "body": "body",
            "acceptance_criteria": ["criterion"],
            "test_plan": "run test",
            "correctness": "matches spec",
            "executor_role": "rust-backend-dev"
        }],
        "loops": [{
            "id": "loop_1",
            "target": "user accepts visuals",
            "budget": 3,
            "count": 1,
            "status": "running",
            "last_iteration_at": now_ms.saturating_sub(STALLED_LOOP_AFTER_MS + 1)
        }]
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    assert!(findings.iter().any(|f| f.kind == "stalled_running_loop"));
}

#[test]
fn quality_warns_for_exhausted_loop_marked_running() {
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "mode": "approved",
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_1",
        "sections": [{"id": "sec_1", "title": "Execution", "body": "section"}],
        "tasks": [{
            "id": "task_1",
            "name": "Do work",
            "type": "implement",
            "status": "pending",
            "section_ids": ["sec_1"],
            "body": "body",
            "acceptance_criteria": ["criterion"],
            "test_plan": "run test",
            "correctness": "matches spec",
            "executor_role": "rust-backend-dev"
        }],
        "loops": [{
            "id": "loop_1",
            "target": "user accepts visuals",
            "budget": 3,
            "count": 3,
            "status": "running"
        }]
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    assert!(findings
        .iter()
        .any(|f| f.kind == "exhausted_loop_still_running"));
}

#[test]
fn quality_warns_for_missing_executor_role_on_execution_task() {
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "mode": "approved",
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_1",
        "sections": [{"id": "sec_1", "title": "Execution", "body": "section"}],
        "tasks": [{
            "id": "task_1",
            "name": "Do work",
            "type": "implement",
            "status": "pending",
            "section_ids": ["sec_1"],
            "body": "body",
            "acceptance_criteria": ["criterion"],
            "test_plan": "run test",
            "correctness": "matches spec"
        }]
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    let warn = findings.iter().find(|f| f.kind == "missing_executor_role");
    assert!(warn.is_some(), "expected missing_executor_role warning");
    assert_eq!(warn.unwrap().severity, "warning");
}

#[test]
fn quality_exempts_planning_kind_from_executor_role_check() {
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "mode": "approved",
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_1",
        "sections": [{"id": "sec_1", "title": "Spec", "body": "section"}],
        "tasks": [{
            "id": "task_1",
            "name": "Author spec",
            "type": "planning",
            "status": "pending",
            "section_ids": ["sec_1"],
            "body": "body",
            "acceptance_criteria": ["criterion"],
            "test_plan": "n/a",
            "correctness": "matches spec"
        }]
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    assert!(
        !findings.iter().any(|f| f.kind == "missing_executor_role"),
        "planning-kind tasks must be exempt from executor_role check"
    );
}

#[test]
fn quality_accepts_externalized_task_body_via_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let tasks_dir = dir.path().join("tasks");
    std::fs::create_dir_all(&tasks_dir).unwrap();
    std::fs::write(
        tasks_dir.join("do-work.md"),
        "---\nplan_id: pl_1\nkind: task\n---\n\nThis is the externalized task body content.\n",
    )
    .unwrap();
    let plan_path = dir.path().join("plan.json");
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "mode": "approved",
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_1",
        "sections": [{"id": "sec_1", "title": "Execution", "body": "section body"}],
        "tasks": [{
            "id": "task_1",
            "name": "Do work",
            "status": "pending",
            "handle": "do-work",
            "section_ids": ["sec_1"],
            "body": "",
            "acceptance_criteria": ["criterion"],
            "test_plan": "run focused test",
            "correctness": "matches spec",
            "executor_role": "frontend-dev"
        }]
    });
    let findings = inspect_plan(&plan_path, &plan, "quality");
    assert!(findings.is_empty(), "{findings:?}");
}

fn planner_pairing_base_plan(planner: Option<Value>) -> Value {
    let mut plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "mode": "approved",
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_1",
        "owner": {"pane_id": "orch-1"},
        "sections": [{"id": "sec_1", "title": "Execution", "body": "section"}],
        "tasks": []
    });
    if let Some(planner) = planner {
        plan["planner"] = planner;
    }
    plan
}

#[test]
fn planner_unpaired_warns_when_orchestrator_bound_without_planner() {
    let plan = planner_pairing_base_plan(None);
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    let finding = findings
        .iter()
        .find(|f| f.kind == "planner_unpaired")
        .expect("expected planner_unpaired finding");
    assert_eq!(finding.severity, "warning");
}

#[test]
fn planner_unpaired_escalates_to_error_at_approval() {
    let dir = tempfile::tempdir().unwrap();
    let plan_dir = dir.path().join(".codeterm/plans/active/pl_1");
    std::fs::create_dir_all(&plan_dir).unwrap();
    let plan = planner_pairing_base_plan(None);
    std::fs::write(
        plan_dir.join("plan.json"),
        serde_json::to_string_pretty(&plan).unwrap(),
    )
    .unwrap();

    let report = doctor_report(
        Some("pl_1".into()),
        Some(dir.path().display().to_string()),
        "quality",
    )
    .unwrap();
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["kind"] == "planner_unpaired" && f["severity"] == "warning"));

    let mut escalated = report.clone();
    escalate_planner_unpaired_for_approval(&mut escalated);
    assert!(!escalated["ok"].as_bool().unwrap());
    assert!(escalated["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["kind"] == "planner_unpaired" && f["severity"] == "error"));
}

#[test]
fn planner_unpaired_clean_when_paired() {
    let plan = planner_pairing_base_plan(Some(json!({
        "kind": "pane",
        "pane_id": "planner-1"
    })));
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    assert!(!findings.iter().any(|f| f.kind == "planner_unpaired"));
}

#[test]
fn planner_unpaired_clean_when_waived() {
    let plan = planner_pairing_base_plan(Some(json!({
        "kind": "waived",
        "note": "solo effort"
    })));
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    assert!(!findings.iter().any(|f| f.kind == "planner_unpaired"));
}

#[test]
fn planner_unpaired_silent_without_orchestrator() {
    let mut plan = planner_pairing_base_plan(None);
    plan["owner"] = json!({});
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    assert!(!findings.iter().any(|f| f.kind == "planner_unpaired"));
}

#[test]
fn quality_errors_for_missing_body_without_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let plan_path = dir.path().join("plan.json");
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "mode": "approved",
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_1",
        "sections": [{"id": "sec_1", "title": "Execution", "body": "section body"}],
        "tasks": [{
            "id": "task_1",
            "name": "Do work",
            "status": "pending",
            "handle": "do-work",
            "section_ids": ["sec_1"],
            "body": "",
            "acceptance_criteria": ["criterion"],
            "test_plan": "run focused test",
            "correctness": "matches spec"
        }]
    });
    let findings = inspect_plan(&plan_path, &plan, "quality");
    assert!(
        findings.iter().any(|f| f.kind == "task_quality_gap"
            && f.task_id.as_deref() == Some("task_1")
            && f.detail.contains("body")),
        "expected task_quality_gap for missing body: {findings:?}"
    );
}
