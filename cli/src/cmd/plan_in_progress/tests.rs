use super::*;

#[test]
fn in_progress_keeps_full_wrapped_task_and_lane_fields_in_json_and_text() {
    let row = json!({"display_key":"K-93", "plan_id":"p-α", "plan_title":"fragment-β",
    "task":{"id":"t-93", "name":"fragment-γ", "status":"in_progress",
        "lane":{"tab_id":"tab-93", "workspace_id":"ws-93", "branch":"branch-93", "cleanup_status":"retained"}}});
    let mut client = FakeRpc::responding_with(json!({"tasks":[row.clone()]}));
    let value = run(&mut client, true, Some("p-α".into()), false)
        .unwrap()
        .unwrap();
    assert_eq!(value["tasks"][0], row);
    assert!(render_in_progress_tasks(&value)
        .contains("t-93\tin_progress\ttab-93\tws-93\tbranch-93\tretained\tfragment-γ\n"));
}
use crate::client::SocketCallError;

#[derive(Default)]
struct FakeRpc {
    calls: Vec<(String, Option<Value>)>,
    response: Value,
}

impl FakeRpc {
    fn responding_with(response: Value) -> Self {
        Self {
            response,
            ..Default::default()
        }
    }

    fn params(&self) -> &Value {
        self.calls[0].1.as_ref().expect("task.list params")
    }
}

impl RpcClient for FakeRpc {
    fn call_detailed(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, SocketCallError> {
        self.calls.push((method.to_string(), params));
        Ok(self.response.clone())
    }
}

#[test]
fn explicit_address_scope_named_plan_ignores_ambient_project() {
    let mut client = FakeRpc::responding_with(json!({ "tasks": [] }));

    run(&mut client, true, Some("pl_foreign".into()), false).unwrap();

    assert_eq!(client.params()["plan_id"], "pl_foreign");
    assert!(client.params().get("project").is_none());
}

#[test]
fn explicit_address_scope_unnamed_plan_keeps_ambient_project() {
    let mut client = FakeRpc::responding_with(json!({ "tasks": [] }));

    run(&mut client, true, None, false).unwrap();

    assert!(client.params()["project"].as_str().is_some());
    assert!(client.params().get("plan_id").is_none());
}

#[test]
fn explicit_address_scope_unreachable_named_plan_is_not_an_empty_negative() {
    let reason = json!({ "kind": "plan_not_found", "plan_id": "pl_missing" });
    let mut client = FakeRpc::responding_with(json!({
        "tasks": [],
        "empty_reason": reason,
    }));

    let value = run(&mut client, true, Some("pl_missing".into()), false)
        .unwrap()
        .expect("json output");

    assert_eq!(value["empty_reason"], reason);
    assert_eq!(
        render_in_progress_tasks(&value),
        "Plan pl_missing was not found.\n"
    );
}

#[test]
fn renders_in_progress_tasks_with_subtask_indentation_and_assignee() {
    let rendered = render_in_progress_tasks(&json!({
        "tasks": [
            {
                "plan_id": "pl_1",
                "plan_title": "Plan",
                "task": {
                    "id": "task_parent",
                    "name": "Parent",
                    "status": "in_progress",
                    "assigned_tab_id": "tab1234",
                    "lane": {
                        "workspace_id": "ws1",
                        "branch": "lane/one",
                        "cleanup_status": "pending"
                    }
                }
            },
            {
                "plan_id": "pl_1",
                "plan_title": "Plan",
                "task": {
                    "id": "task_child",
                    "name": "Child",
                    "status": "blocked",
                    "parent_task_id": "task_parent"
                }
            }
        ]
    }));

    assert!(rendered.contains("pl_1\tPlan\n"));
    assert!(
        rendered.contains("  task_parent\tin_progress\ttab1234\tws1\tlane/one\tpending\tParent\n")
    );
    assert!(rendered.contains("    task_child\tblocked\t-\t-\t-\t-\tChild\n"));
}
