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
fn renders_file_manifest() {
    let rendered = render_file_manifest(&json!({
        "index_path": ".codeterm/plans/pl_1/INDEX.md",
        "entries": [
            {
                "kind": "section",
                "title": "Spec",
                "path": ".codeterm/plans/pl_1/sections/sec_1.md"
            },
            {
                "kind": "task",
                "title": "Build",
                "path": ".codeterm/plans/pl_1/tasks/task_1.md"
            }
        ]
    }));
    assert!(rendered.contains("INDEX.md\t.codeterm/plans/pl_1/INDEX.md"));
    assert!(rendered.contains("Spec\t.codeterm/plans/pl_1/sections/sec_1.md"));
    assert!(rendered.contains("Build\t.codeterm/plans/pl_1/tasks/task_1.md"));
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
