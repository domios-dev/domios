use super::*;
use serde_json::json;

#[test]
fn simple_task_list_indents_subtasks_under_parent() {
    let rendered = render_simple_task_list(&[
        json!({
            "plan_id": "pl_1",
            "task": { "id": "task_parent", "name": "Parent", "status": "in_progress" }
        }),
        json!({
            "plan_id": "pl_1",
            "task": {
                "id": "task_child",
                "name": "Child",
                "status": "pending",
                "parent_task_id": "task_parent"
            }
        }),
    ]);
    assert!(rendered.contains("task_parent\t-\tin_progress\tpl_1\tParent\n"));
    assert!(rendered.contains("  task_child\t-\tpending\tpl_1\tChild\n"));
}

#[test]
fn task_key_contract_list_row_prints_daemon_display_key() {
    let rendered = render_simple_task_list(&[
        json!({
            "plan_id": "pl_1",
            "display_key": "COD-7",
            "task": { "id": "task_new001", "key": "COD-7", "name": "New", "status": "pending" }
        }),
        json!({
            "plan_id": "pl_1",
            "display_key": "OLD001",
            "task": { "id": "task_old001", "name": "Old", "status": "pending" }
        }),
    ]);
    assert!(
        rendered.contains("task_new001\tCOD-7\tpending\tpl_1\tNew\n"),
        "{rendered}"
    );
    assert!(
        rendered.contains("task_old001\tOLD001\tpending\tpl_1\tOld\n"),
        "{rendered}"
    );
}

#[test]
fn task_key_contract_key_label_reads_summary_detail_and_bare_task_json() {
    assert_eq!(
        task_key_label(&json!({ "display_key": "COD-7", "task": { "id": "task_a" } })),
        "COD-7"
    );
    assert_eq!(
        task_key_label(&json!({ "id": "task_a", "key": "COD-8" })),
        "COD-8"
    );
    assert_eq!(task_key_label(&json!({ "id": "task_a" })), "-");
}

#[test]
fn simple_task_list_renders_external_references() {
    let rendered = render_simple_task_list(&[json!({
        "plan_id": "pl_1",
        "task": {
            "id": "task_1",
            "name": "Linked",
            "status": "pending",
            "external_refs": [{
                "kind": "github_issue",
                "repository": "rollacode/codeterm-releases",
                "number": 17,
                "canonical_url": "https://github.com/rollacode/codeterm-releases/issues/17"
            }]
        }
    })]);
    assert!(rendered.contains("https://github.com/rollacode/codeterm-releases/issues/17"));
}
