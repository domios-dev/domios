use super::*;
use serde_json::json;

#[test]
fn plan_delete_refuses_without_confirmation_before_rpc() {
    let err = plan_delete_params("pl_1".into(), false).unwrap_err();
    assert!(err.contains("refusing to delete plan 'pl_1' without --yes"));
    assert!(err.contains("--confirm"));
}

#[test]
fn plan_delete_confirmation_builds_rpc_params() {
    assert_eq!(
        plan_delete_params("pl_1".into(), true).unwrap(),
        json!({ "plan_id": "pl_1" })
    );
}

#[test]
fn plan_transfer_params_require_agent_caller() {
    let agent = plan_transfer_params(
        "pl_1".into(),
        "destination".into(),
        Some("handoff".into()),
        Some("source-manager"),
    )
    .unwrap();
    assert_eq!(agent["caller_pane_id"], "source-manager");
    assert_eq!(agent["to"]["pane_id"], "destination");
    assert_eq!(agent["reason"], "handoff");

    let missing =
        plan_transfer_params("pl_1".into(), "destination".into(), None, None).unwrap_err();
    assert!(missing.contains("CODETERM_PANE_ID"));
}

#[test]
fn renders_undeclared_kind_fix() {
    let msg = render_plan_error(
        &json!({ "type": "undeclared_kind", "declared": ["status_update", "review"] }),
        &json!({ "plan_id": "plan_1", "task_id": "task_1" }),
    )
    .unwrap();
    assert!(msg.contains("✗ undeclared milestone kind"));
    assert!(msg.contains("valid kinds: status_update|review"));
    assert!(msg.contains("codeterm plan kinds set plan_1 --kinds status_update,review,<new>"));
    assert!(msg.contains("--add-kind"));
}

#[test]
fn renders_bad_status_fix() {
    let msg = render_plan_error(
        &json!({ "type": "bad_status" }),
        &json!({ "plan_id": "plan_1", "task_id": "task_1" }),
    )
    .unwrap();
    assert!(msg.contains("valid task statuses"));
    assert!(msg.contains("pending|in_progress|in_review|blocked|paused|done|cancelled"));
    assert!(msg.contains("codeterm plan task update plan_1 task_1 --status pending"));
    assert!(msg.contains("--status-label"));
}

#[test]
fn renders_task_pause_reason_fix() {
    let msg = render_plan_error(
        &json!({ "type": "task_pause_requires_reason" }),
        &json!({ "plan_id": "plan_1", "task_id": "task_1" }),
    )
    .unwrap();
    assert!(msg.contains("codeterm plan task pause plan_1 task_1 --reason"));
}

#[test]
fn renders_plan_bad_status_fix() {
    let msg = render_plan_error(
        &json!({ "type": "bad_status" }),
        &json!({ "plan_id": "plan_1", "status": "closed" }),
    )
    .unwrap();
    assert!(msg.contains("valid plan statuses: active|archived"));
    assert!(msg.contains("codeterm plan update plan_1 --status active"));
}

#[test]
fn renders_bad_mode_fix() {
    let msg = render_plan_error(
        &json!({ "type": "bad_mode" }),
        &json!({ "plan_id": "plan_1", "mode": "running" }),
    )
    .unwrap();
    assert!(msg.contains("valid modes: planning|approved|execution|complete"));
    assert!(msg.contains("codeterm plan mode plan_1 --set planning"));
    assert!(msg.contains("codeterm plan approve plan_1"));
}

#[test]
fn renders_bad_priority_fix() {
    let msg = render_plan_error(
        &json!({ "type": "bad_priority" }),
        &json!({ "plan_id": "plan_1", "task_id": "task_1", "priority": "urgent" }),
    )
    .unwrap();
    assert!(msg.contains("valid priorities: p0|p1|p2|p3"));
    assert!(msg.contains("codeterm plan task update plan_1 task_1 --priority p2"));
}

#[test]
fn renders_unknown_plan_fix() {
    let msg = render_plan_error(
        &json!({ "type": "not_found" }),
        &json!({ "plan_id": "missing" }),
    )
    .unwrap();
    assert!(msg.contains("✗ no plan 'missing'"));
    assert!(msg.contains("codeterm plan list --active"));
    assert!(msg.contains("env -u CODETERM_API_TOKEN CODETERM_API_PORT=7686 codeterm"));
}

#[test]
fn renders_unknown_spec_fix() {
    let msg = render_plan_error(
        &json!({ "type": "not_found" }),
        &json!({ "spec_id": "spec_missing" }),
    )
    .unwrap();
    assert!(msg.contains("✗ no spec 'spec_missing'"));
    assert!(msg.contains("codeterm spec list --active"));
}

#[test]
fn renders_unknown_plan_section_fix() {
    let msg = render_plan_error(
        &json!({ "type": "not_found" }),
        &json!({ "plan_id": "plan_1", "section_id": "sec_missing" }),
    )
    .unwrap();
    assert!(msg.contains("✗ no section 'sec_missing' was found in plan 'plan_1'"));
    assert!(msg.contains("codeterm plan get plan_1 --sections"));
}

#[test]
fn renders_unknown_spec_section_fix() {
    let msg = render_plan_error(
        &json!({ "type": "not_found" }),
        &json!({ "spec_id": "spec_1", "section_id": "sec_missing" }),
    )
    .unwrap();
    assert!(msg.contains("✗ no section 'sec_missing' was found in spec 'spec_1'"));
    assert!(msg.contains("codeterm spec show spec_1"));
}

#[test]
fn renders_unknown_loop_fix() {
    let msg = render_plan_error(
        &json!({ "type": "not_found" }),
        &json!({ "loop_id": "loop_missing" }),
    )
    .unwrap();
    assert!(msg.contains("✗ no loop 'loop_missing'"));
    assert!(msg.contains("codeterm plan state <plan_id>"));
}

#[test]
fn renders_unknown_heartbeat_plan_fix() {
    let msg = render_plan_error(
        &json!({ "type": "not_found" }),
        &json!({ "plan_id": null, "pane_id": "pane_1", "session_id": "session_1" }),
    )
    .unwrap();
    assert!(msg.contains("✗ no heartbeat plan was found for this tab/session"));
    assert!(msg.contains("codeterm orchestrator heartbeat show --plan <id>"));
    assert!(!msg.contains("heartbeat set"));
}

#[test]
fn renders_unknown_task_fix() {
    let msg = render_plan_error(
        &json!({ "type": "task_not_found" }),
        &json!({ "plan_id": "plan_1", "task_id": "missing_task" }),
    )
    .unwrap();
    assert!(msg.contains("✗ no task 'missing_task'"));
    assert!(msg.contains("codeterm plan list --active"));
    assert!(msg.contains("env -u CODETERM_API_TOKEN CODETERM_API_PORT=7686 codeterm"));
}

#[test]
fn renders_io_with_generic_plan_fix() {
    let msg = render_plan_error(
        &json!({ "type": "io", "message": "permission denied" }),
        &json!({ "plan_id": "plan_1" }),
    )
    .unwrap();
    assert!(msg.contains("✗ planner I/O failed: permission denied"));
    assert!(msg.contains("codeterm plan get plan_1 --json"));
    assert!(msg.contains("codeterm plan files plan_1"));
}

#[test]
fn renders_io_with_handoff_fix() {
    let msg = render_plan_error(
        &json!({ "type": "io", "message": "handoff content must be an object" }),
        &json!({ "plan_id": "plan_1", "selector": "latest" }),
    )
    .unwrap();
    assert!(msg.contains("✗ handoff failed: handoff content must be an object"));
    assert!(msg.contains("codeterm plan handoff list plan_1"));
    assert!(msg.contains("codeterm plan handoff show plan_1 --latest --markdown"));
}

#[test]
fn renders_unknown_element_fix() {
    let msg = render_plan_error(
        &json!({ "type": "element_not_found" }),
        &json!({ "plan_id": "plan_1", "element_id": "missing_element" }),
    )
    .unwrap();
    assert!(msg.contains("✗ no element 'missing_element'"));
    assert!(msg.contains("codeterm plan element list plan_1"));
}

#[test]
fn renders_in_review_done_transition_fix() {
    let msg = render_plan_error(
        &json!({
            "type": "transition_denied",
            "from": "in_review",
            "to": "done"
        }),
        &json!({ "plan_id": "plan_1", "task_id": "task_1" }),
    )
    .unwrap();
    assert!(msg.contains("workers cannot mark an in-review task done"));
    assert!(msg.contains("Orchestrator must accept it explicitly"));
    assert!(msg.contains("codeterm plan task close plan_1 task_1"));
    assert!(msg.contains("--status-label \"needs changes\""));
}

#[test]
fn plan_tagged_pane_not_found_gets_cross_daemon_hint() {
    let err = crate::client::SocketCallError::Rpc(crate::client::RpcError {
        code: -32603,
        message: "Tab not found".into(),
        data: None,
    });
    let msg = render_plan_call_error(err, &json!({ "plan_id": "pl_dev", "task_id": "task_dev" }));
    assert!(msg.contains("RPC error -32603: Tab not found"));
    assert!(msg.contains("env -u CODETERM_API_TOKEN CODETERM_API_PORT=7686 codeterm"));
}

#[test]
fn renders_loop_budget_fix() {
    let msg = render_plan_error(
        &json!({ "type": "budget_exhausted", "count": 3, "budget": 3 }),
        &json!({ "loop_id": "loop_1" }),
    )
    .unwrap();
    assert!(msg.contains("✗ loop budget 3/3 reached"));
    assert!(msg.contains("codeterm plan loop set loop_1 --budget N"));
}

#[test]
fn renders_write_pending_without_blind_retry() {
    let msg = render_plan_error(
        &json!({
            "type": "write_pending",
            "operation": "plan_index_mirror",
            "committed": true,
            "committed_id": "task_committed",
            "message": "database is locked"
        }),
        &json!({ "plan_id": "plan_1" }),
    )
    .unwrap();
    assert!(msg.contains("pending after a committed write"));
    assert!(msg.contains("do not retry blindly"));
    assert!(msg.contains("task_committed"));
    assert!(msg.contains("codeterm task show task_committed"));
}

#[test]
fn renders_readback_mismatch_without_blind_retry() {
    let msg = render_plan_error(
        &json!({
            "type": "readback_mismatch",
            "operation": "task_update",
            "message": "status did not match"
        }),
        &json!({ "plan_id": "plan_1", "task_id": "task_1" }),
    )
    .unwrap();
    assert!(msg.contains("did not read back"));
    assert!(msg.contains("do not retry blindly"));
    assert!(msg.contains("codeterm task show task_1"));
}

#[test]
fn all_backend_plan_error_types_have_cli_renderers() {
    let source = include_str!("../../../../src-tauri/src/service/plan/types.rs");
    let mut types = source
        .lines()
        .filter_map(extract_plan_error_type)
        .collect::<Vec<_>>();
    types.sort();
    types.dedup();
    assert!(!types.is_empty());

    for kind in types {
        let data = json!({
            "type": kind,
            "from": "in_progress",
            "to": "done",
            "declared": ["status_update"],
            "count": 1,
            "budget": 2,
            "operation": "test_operation",
            "committed": true,
            "committed_id": "task_1",
            "message": "test message",
            "subtasks": []
        });
        let params = json!({
            "plan_id": "plan_1",
            "task_id": "task_1",
            "loop_id": "loop_1",
            "section_id": "section_1"
        });
        let msg = render_plan_error(&data, &params)
            .unwrap_or_else(|| panic!("missing CLI renderer for PlanError type '{kind}'"));
        assert!(
            !msg.contains("RPC error"),
            "renderer for PlanError type '{kind}' fell back to raw RPC: {msg}"
        );
    }
}

#[test]
fn group_not_found_renderer_points_at_group_and_qa_commands() {
    let msg = render_plan_error(
        &json!({ "type": "group_not_found" }),
        &json!({ "plan_id": "pl_1", "group": "phase-1" }),
    )
    .expect("group_not_found renderer");
    assert!(msg.contains("task group"));
    assert!(msg.contains("codeterm plan groups set pl_1"));
    assert!(msg.contains("codeterm plan qa next pl_1 --group"));
}

#[test]
fn ref_ambiguous_renderer_lists_candidates() {
    let msg = render_plan_error(
        &json!({
            "type": "ref_ambiguous",
            "kind": "task",
            "reference": "auth",
            "candidates": [
                { "id": "task_auth_login", "label": "Login" },
                { "id": "task_auth_logout", "label": "Logout" }
            ]
        }),
        &json!({ "plan_id": "pl_1", "task_id": "auth" }),
    )
    .expect("ref_ambiguous renderer");
    assert!(msg.contains("ambiguous"));
    assert!(msg.contains("task_auth_login"));
    assert!(msg.contains("task_auth_logout"));
}

#[test]
fn ref_not_found_renderer_is_actionable() {
    let msg = render_plan_error(
        &json!({
            "type": "ref_not_found",
            "kind": "plan",
            "reference": "pl_missing"
        }),
        &json!({ "plan_id": "pl_missing" }),
    )
    .expect("ref_not_found renderer");
    assert!(msg.contains("not found"));
    assert!(msg.contains("pl_missing"));
    assert!(msg.contains("codeterm plan list --active"));
}

fn extract_plan_error_type(line: &str) -> Option<String> {
    let marker = "\"type\"";
    let idx = line.find(marker)?;
    let rest = &line[idx + marker.len()..];
    let colon = rest.find(':')?;
    let rest = rest[colon + 1..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

#[test]
fn render_plan_markdown_prepends_goal_block() {
    let rendered = render_plan_markdown(&json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "goal": {
            "vector": "Ship the foundation",
            "vision": "Markdown vision",
            "constraints": ["backend only"],
            "postulates": ["reread plan"]
        },
        "sections": [],
        "tasks": []
    }));
    let goal_pos = rendered.find("## Goal").unwrap();
    let sections_pos = rendered.find("## Sections").unwrap_or(rendered.len());
    assert!(goal_pos < sections_pos);
    assert!(rendered.contains("CodeTerm planner state is authoritative"));
    assert!(rendered.contains("**Vector:** Ship the foundation"));
    assert!(rendered.contains("- backend only"));
    assert!(rendered.contains("- reread plan"));
    assert!(rendered.contains("Markdown vision"));
}

#[test]
fn render_plan_markdown_renders_task_quality_fields() {
    let rendered = render_plan_markdown(&json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "tasks": [
            {
                "id": "task_1",
                "name": "Write tests",
                "status": "in_review",
                "acceptance_criteria": ["hints covered", "heartbeat covered"],
                "test_plan": "cargo test focused modules",
                "correctness": "formatter contracts are stable"
            }
        ]
    }));

    assert!(rendered.contains("- **Write tests** (`task_1`)"));
    assert!(rendered.contains("  - criterion: hints covered"));
    assert!(rendered.contains("  - criterion: heartbeat covered"));
    assert!(rendered.contains("  - test plan: cargo test focused modules"));
    assert!(rendered.contains("  - correctness: formatter contracts are stable"));
}

#[test]
fn render_plan_markdown_indents_subtasks_under_parent() {
    let rendered = render_plan_markdown(&json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "tasks": [
            {
                "id": "task_parent",
                "name": "Parent",
                "status": "in_progress"
            },
            {
                "id": "task_child",
                "name": "Child",
                "status": "pending",
                "parent_task_id": "task_parent"
            }
        ]
    }));

    assert!(rendered.contains("- **Parent** (`task_parent`) — in_progress\n"));
    assert!(rendered.contains("  - **Child** (`task_child`) — pending\n"));
}

#[test]
fn render_plan_markdown_omits_empty_goal_and_task_quality_fields() {
    let rendered = render_plan_markdown(&json!({
        "id": "pl_1",
        "title": "Plan",
        "status": "active",
        "goal": null,
        "tasks": [
            {
                "id": "task_1",
                "name": "Write tests",
                "status": "pending",
                "acceptance_criteria": [],
                "test_plan": "",
                "correctness": ""
            }
        ]
    }));

    assert!(!rendered.contains("## Goal"));
    assert!(rendered.contains("- **Write tests** (`task_1`)"));
    assert!(!rendered.contains("criterion:"));
    assert!(!rendered.contains("test plan:"));
    assert!(!rendered.contains("correctness:"));
}
