//! Doctor checks that judge plan QUALITY: approval gates, task completeness,
//! cleanup debt, loop health, planner pairing, and test-plan executability.
//! Structure and storage checks stay in the parent module.

use std::time::{SystemTime, UNIX_EPOCH};

use super::super::quality::inspect_task_lifecycle;
use super::super::*;
use super::write_v2_plan;

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
            "type": "planning",
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
            "type": "implement",
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
fn structure_ready_plan_with_only_planning_task_passes_approval_gate() {
    // Issue #231 finding 3: a structure_ready plan whose only task is
    // planning-kind (stored as "type": "planning") must not trip the gate.
    let plan = json!({
        "id": "pl_b5g9vg",
        "title": "SCRATCH-owner-plan",
        "status": "active",
        "mode": "planning",
        "planning_state": "structure_ready",
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_qtj969",
        "sections": [{"id": "sec_1", "title": "Spec", "body": "WHAT/WHY"}],
        "tasks": [{
            "id": "task_2maz43",
            "name": "scratch planning task",
            "type": "planning",
            "status": "pending",
            "section_ids": ["sec_1"]
        }]
    });
    let findings = inspect_plan(Path::new("pl_b5g9vg.json"), &plan, "quality");
    assert!(
        !findings.iter().any(|f| f.kind == "tasks_before_approval"),
        "structure_ready plan with only a planning-kind task must not trip the approval gate: {findings:?}"
    );
}

#[test]
fn from_spec_owned_plan_not_flagged_missing_spec_source() {
    // A sectionless spec still needs ownership metadata so the spec-source check can prove the relationship.
    let plan = json!({
        "id": "pl_b5g9vg",
        "title": "SCRATCH-owner-plan",
        "status": "active",
        "mode": "planning",
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_qtj969",
        "spec_refs": ["sp_qtj969"],
        "spec_links": [],
        "sections": [],
        "tasks": []
    });
    let findings = inspect_plan(Path::new("pl_b5g9vg.json"), &plan, "quality");
    assert!(
        !findings.iter().any(|f| f.kind == "missing_spec_source"),
        "a --from-spec-owned plan must not be flagged missing_spec_source: {findings:?}"
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
                "cleanup_hints": ["waiting for shared dev Tab shutdown"]
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
fn quality_errors_for_paused_without_reason() {
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
            "status": "paused",
            "section_ids": ["sec_1"],
            "body": "b",
            "acceptance_criteria": ["c"],
            "test_plan": "t",
            "correctness": "x",
            "executor_role": "rust-backend-dev"
        }]
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    assert!(findings.iter().any(|f| f.kind == "paused_without_reason"
        && f.task_id.as_deref() == Some("task_1")
        && f.severity == "error"));
}

#[test]
fn quality_preserves_provider_exhausted_pause() {
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "mode": "execution",
        "goal": {"vector": "ship"},
        "tasks": [{
            "id": "task_1",
            "name": "Do work",
            "status": "paused",
            "status_label": "provider_exhausted",
            "section_ids": [],
            "body": "b",
            "acceptance_criteria": ["c"],
            "test_plan": "t",
            "correctness": "x",
            "executor_role": "rust-backend-dev"
        }]
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    assert!(!findings.iter().any(|finding| {
        finding.kind == "paused_without_reason" && finding.task_id.as_deref() == Some("task_1")
    }));
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
            "test_plan": "Run `cargo test plan::scoped_change` and assert the new path is covered; then `codeterm plan doctor pl_1 --kind quality` round-trips with zero findings.",
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
fn quality_errors_for_test_author_report_without_red_green_demo() {
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "mode": "approved",
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_1",
        "sections": [{"id": "sec_1", "title": "Tests", "body": "section"}],
        "tasks": [{
            "id": "task_1",
            "name": "Author drill",
            "type": "test",
            "status": "done",
            "section_ids": ["sec_1"],
            "body": "body",
            "acceptance_criteria": ["criterion"],
            "test_plan": "run cargo test drill_fixture",
            "correctness": "matches spec",
            "executor_role": "test-author",
            "lane": {
                "report": {
                    "id": "report_1",
                    "message": "STATUS: done\nCHANGES: added drill\nTESTS RUN: cargo test drill_fixture passed"
                }
            }
        }]
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    let finding = findings
        .iter()
        .find(|f| f.kind == "test_author_report_missing_red_green_demo");

    assert!(finding.is_some(), "expected red-green report enforcement");
    assert_eq!(finding.unwrap().severity, "error");
}

#[test]
fn quality_accepts_test_author_report_with_red_green_demo() {
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "mode": "approved",
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_1",
        "sections": [{"id": "sec_1", "title": "Tests", "body": "section"}],
        "tasks": [{
            "id": "task_1",
            "name": "Author drill",
            "type": "test",
            "status": "done",
            "section_ids": ["sec_1"],
            "body": "body",
            "acceptance_criteria": ["criterion"],
            "test_plan": "run cargo test drill_fixture",
            "correctness": "matches spec",
            "executor_role": "test-author",
            "lane": {
                "report": {
                    "id": "report_1",
                    "message": "STATUS: done\nRED-GREEN DEMO: red: sabotaged readiness gate failed on oracle; green: restored gate passed.\nTESTS RUN: cargo test drill_fixture"
                }
            }
        }]
    });
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");

    assert!(
        !findings
            .iter()
            .any(|f| f.kind == "test_author_report_missing_red_green_demo"),
        "red-green evidence should satisfy test-author report gate: {findings:?}"
    );
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
            "test_plan": "Run `cargo test plan::externalized_body` and assert the sidecar body is read; then `codeterm plan doctor pl_1 --kind quality` round-trips with zero findings.",
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
        "owner": {"tab_id": "orch-1"},
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
    assert!(finding.fix.contains("agent spawn <provider>"));
    assert!(finding.fix.contains("[--model <model>]"));
    assert!(!finding.fix.contains("agent spawn claude"));
    assert!(!finding.fix.contains("fable"));
}

#[test]
fn planner_unpaired_escalates_to_error_at_approval() {
    let dir = tempfile::tempdir().unwrap();
    let plan_dir = dir.path().join(".codeterm/plans/active/pl_1");
    let plan = planner_pairing_base_plan(None);
    write_v2_plan(&plan_dir, &plan);

    let report = super::authoritative_report(&plan, "pl_1", dir.path(), "quality");
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["kind"] == "planner_unpaired" && f["severity"] == "warning"));

    let mut escalated = report.clone();
    escalate_approval_blocking_warnings(&mut escalated);
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
        "kind": "tab",
        "tab_id": "planner-1"
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

fn task_with_test_plan(kind: &str, test_plan: &str) -> Value {
    json!({
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
            "type": kind,
            "status": "pending",
            "section_ids": ["sec_1"],
            "body": "body",
            "acceptance_criteria": ["criterion"],
            "test_plan": test_plan,
            "correctness": "matches spec",
            "executor_role": "rust-backend-dev"
        }]
    })
}

#[test]
fn quality_warns_on_under_length_test_plan() {
    let plan = task_with_test_plan("implement", "component tests; manual check");
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    assert!(findings
        .iter()
        .any(|f| f.kind == "test_plan_vague" && f.severity == "warning"));
}

#[test]
fn quality_warns_on_commandless_test_plan() {
    let wordy = "We will carefully verify the resulting behavior by inspecting the output and \
confirming that every relevant surface looks correct across the whole flow before sign-off.";
    let plan = task_with_test_plan("implement", wordy);
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    assert!(findings.iter().any(|f| f.kind == "test_plan_vague"));
}

#[test]
fn test_plan_vague_finding_names_failed_criterion_and_tokens() {
    let plan = task_with_test_plan("implement", "`cargo test`");
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    let finding = findings
        .iter()
        .find(|f| f.kind == "test_plan_vague")
        .expect("vague finding present");
    assert!(finding.detail.contains("too short"), "{}", finding.detail);
    assert!(
        !finding.detail.contains("no runnable command"),
        "{}",
        finding.detail
    );

    let wordy = "We will carefully verify the resulting behavior by inspecting the output and \
confirming that every relevant surface looks correct across the whole flow before sign-off.";
    let plan = task_with_test_plan("implement", wordy);
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    let finding = findings
        .iter()
        .find(|f| f.kind == "test_plan_vague")
        .expect("vague finding present");
    assert!(
        finding.detail.contains("names no runnable command"),
        "{}",
        finding.detail
    );
    for token in crate::cmd::test_plan::TEST_PLAN_COMMAND_RUNNERS {
        assert!(finding.detail.contains(token), "{}", finding.detail);
    }
}

#[test]
fn quality_clean_on_executable_test_plan() {
    let good =
        "Run `cargo test test_plan_vague` and assert exactly one test_plan_vague finding on \
the seeded plan; then `codeterm plan doctor pl_1 --kind quality` round-trips clean.";
    let plan = task_with_test_plan("implement", good);
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    assert!(!findings.iter().any(|f| f.kind == "test_plan_vague"));
}

#[test]
fn quality_exempts_planning_kind_from_vague_test_plan() {
    let plan = task_with_test_plan("planning", "n/a");
    let findings = inspect_plan(Path::new("pl_1.json"), &plan, "quality");
    assert!(
        !findings.iter().any(|f| f.kind == "test_plan_vague"),
        "planning-kind tasks must be exempt from the vague test-plan check"
    );
}

#[test]
fn quality_seeded_plan_flags_exactly_one_vague_test_plan() {
    let dir = tempfile::tempdir().unwrap();
    let plan_dir = dir.path().join(".codeterm/plans/active/pl_1");
    let plan = json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "mode": "approved",
        "goal": {"vector": "ship"},
        "primary_spec_id": "sp_1",
        "sections": [{"id": "sec_1", "title": "Execution", "body": "paths, build.sh check"}],
        "tasks": [
            {
                "id": "task_vague",
                "name": "Vague one",
                "type": "implement",
                "status": "pending",
                "section_ids": ["sec_1"],
                "body": "body",
                "acceptance_criteria": ["criterion"],
                "test_plan": "component tests; manual check",
                "correctness": "matches spec",
                "executor_role": "rust-backend-dev"
            },
            {
                "id": "task_good",
                "name": "Concrete one",
                "type": "implement",
                "status": "pending",
                "section_ids": ["sec_1"],
                "body": "body",
                "acceptance_criteria": ["criterion"],
                "test_plan": "Run `cargo test test_plan_vague` and assert exactly one test_plan_vague finding on the seeded plan; then `codeterm plan doctor pl_1 --kind quality` round-trips clean.",
                "correctness": "matches spec",
                "executor_role": "rust-backend-dev"
            }
        ]
    });
    write_v2_plan(&plan_dir, &plan);

    let report = super::authoritative_report(&plan, "pl_1", dir.path(), "quality");
    let vague: Vec<&Value> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["kind"] == "test_plan_vague")
        .collect();
    assert_eq!(vague.len(), 1, "exactly one vague finding: {vague:?}");
    assert_eq!(vague[0]["task_id"], "task_vague");
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

// Add and doctor must share the missing-field verdict, so both paths use the same helper.
#[test]
fn add_time_missing_body_hint_matches_doctor_verdict() {
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
    let doctor_detail = findings
        .iter()
        .find(|f| f.kind == "task_quality_gap" && f.task_id.as_deref() == Some("task_1"))
        .map(|f| f.detail.clone())
        .expect("doctor flags the missing body");
    assert!(doctor_detail.contains("body"), "{doctor_detail}");
    assert!(
        !doctor_detail.contains("acceptance criteria")
            && !doctor_detail.contains("test plan")
            && !doctor_detail.contains("correctness vector"),
        "doctor should only flag body: {doctor_detail}"
    );

    let add_time_hint =
        crate::cmd::hints::task_quality_guard_hint("pl_1", "task_1", false, true, true, true)
            .expect("add-time hint fires for the same missing body");
    assert!(add_time_hint.contains("body"), "{add_time_hint}");
    assert!(
        !add_time_hint.contains("acceptance criteria")
            && !add_time_hint.contains("test plan")
            && !add_time_hint.contains("correctness vector"),
        "add-time hint should only flag body: {add_time_hint}"
    );
}

// The convergence point: `plan task add` requires a role, but HTTP and socket
// callers never pass through that flag, so approval checks the field again.
#[test]
fn executor_role_required_approval_refuses_a_plan_holding_a_roleless_task() {
    let dir = tempfile::tempdir().unwrap();
    let plan_dir = dir.path().join(".codeterm/plans/active/pl_1");
    write_v2_plan(&plan_dir, &roleless_task_plan());

    let mut report =
        super::authoritative_report(&roleless_task_plan(), "pl_1", dir.path(), "quality");
    assert!(
        report["ok"].as_bool().unwrap(),
        "standalone doctor only reports it: {report}"
    );
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["kind"] == "missing_executor_role" && f["severity"] == "warning"));

    escalate_approval_blocking_warnings(&mut report);
    assert!(!report["ok"].as_bool().unwrap(), "approval must refuse");
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["kind"] == "missing_executor_role" && f["severity"] == "error"));
}

// An invented role is worse than an absent one: absence is honest, a fabricated
// role is something the Orchestrator would route on.
#[test]
fn executor_role_required_preexisting_roleless_tasks_are_reported_not_filled() {
    let dir = tempfile::tempdir().unwrap();
    let plan_dir = dir.path().join(".codeterm/plans/active/pl_1");
    write_v2_plan(&plan_dir, &roleless_task_plan());
    let (report, calls) = super::authoritative_report_with_calls(
        &roleless_task_plan(),
        "pl_1",
        dir.path(),
        "quality",
    );
    let finding = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "missing_executor_role")
        .expect("expected the roleless task to be surfaced");
    assert_eq!(finding["task_id"], "task_1");

    assert!(
        calls.iter().all(|m| m == "plan.get" || m == "plan.files"),
        "the doctor must never write a role in, only read: {calls:?}"
    );
}

#[test]
fn role_guidance_contract_doctor_warns_on_duplicates_without_blocking_similar_guidance() {
    let plan_with = |second_guidance: &str| {
        json!({
            "id": "pl_1",
            "title": "Plan",
            "status": "active",
            "mode": "execution",
            "goal": {"vector": "ship"},
            "primary_spec_id": "sp_1",
            "sections": [{"id": "sec_1", "title": "Work", "body": "WHAT/WHY"}],
            "tasks": [
                {
                    "id": "task_a", "name": "A", "type": "planning", "status": "pending",
                    "section_ids": ["sec_1"], "body": "A", "acceptance_criteria": ["A"],
                    "test_plan": "A", "correctness": "A", "role_guidance": "shared-guidance"
                },
                {
                    "id": "task_b", "name": "B", "type": "planning", "status": "pending",
                    "section_ids": ["sec_1"], "body": "B", "acceptance_criteria": ["B"],
                    "test_plan": "B", "correctness": "B", "role_guidance": second_guidance
                }
            ]
        })
    };

    let duplicate = inspect_plan(
        Path::new("plan.yaml"),
        &plan_with("shared-guidance"),
        "quality",
    );
    let warnings = duplicate
        .iter()
        .filter(|finding| finding.kind == "duplicate_role_guidance")
        .collect::<Vec<_>>();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].severity, "warning");
    assert!(!APPROVAL_BLOCKING_WARNINGS.contains(&"duplicate_role_guidance"));
    assert!(!duplicate.iter().any(|finding| finding.severity == "error"));

    let similar = inspect_plan(
        Path::new("plan.yaml"),
        &plan_with("shared-guidance with a distinct task boundary"),
        "quality",
    );
    assert!(!similar
        .iter()
        .any(|finding| finding.kind == "duplicate_role_guidance"));
}

fn roleless_task_plan() -> Value {
    json!({
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
            "type": "implement",
            "status": "pending",
            "section_ids": ["sec_1"],
            "body": "Implement the scoped change.",
            "acceptance_criteria": ["criterion"],
            "test_plan": "Run `cargo test plan::scoped_change` and assert the new path is covered; then `codeterm plan doctor pl_1 --kind quality` round-trips with zero findings.",
            "correctness": "matches spec"
        }]
    })
}
