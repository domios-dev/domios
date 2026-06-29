use super::*;
use serde_json::json;

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
    assert!(msg.contains("pending|in_progress|blocked|done|cancelled"));
    assert!(msg.contains("codeterm plan task update plan_1 task_1 --status pending"));
    assert!(msg.contains("--status-label"));
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
fn plan_tagged_pane_not_found_gets_cross_daemon_hint() {
    let err = crate::client::SocketCallError::Rpc(crate::client::RpcError {
        code: -32603,
        message: "Pane not found".into(),
        data: None,
    });
    let msg = render_plan_call_error(err, &json!({ "plan_id": "pl_dev", "task_id": "task_dev" }));
    assert!(msg.contains("RPC error -32603: Pane not found"));
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
