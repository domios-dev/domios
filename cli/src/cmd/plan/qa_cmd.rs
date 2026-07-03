use crate::args::plan::PlanQaAction;
use crate::client::SocketClient;
use serde_json::{json, Value};

use super::{call_plan_rpc, maybe_inject_caller_pane, owner_from_env};

pub(super) fn handle(
    client: &mut SocketClient,
    json_output: bool,
    action: PlanQaAction,
) -> Result<Option<Value>, String> {
    match action {
        PlanQaAction::Queue {
            plan_id,
            task,
            note,
        } => {
            let mut params = json!({ "from": owner_from_env() });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            if let Some(task) = task {
                params["task_id"] = json!(task);
            }
            if let Some(note) = note {
                params["note"] = json!(note);
            }
            maybe_inject_caller_pane(&mut params);
            let val = call_plan_rpc(client, "plan.qa.queue", params)?;
            if json_output || val.get("task_id").is_some() {
                return Ok(Some(val));
            }
            print!("{}", render_qa_queue(&val));
            Ok(None)
        }
        PlanQaAction::Next {
            plan_id,
            claim,
            note,
        } => {
            let mut params = json!({ "claim": claim, "from": owner_from_env() });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            if let Some(note) = note {
                params["note"] = json!(note);
            }
            maybe_inject_caller_pane(&mut params);
            let val = call_plan_rpc(client, "plan.qa.next", params)?;
            if json_output {
                return Ok(Some(val));
            }
            if claim {
                print!("{}", render_qa_claim(&val));
            } else {
                println!("{}", val.get("next").cloned().unwrap_or(Value::Null));
            }
            Ok(None)
        }
        PlanQaAction::Pass {
            plan_id,
            task_id,
            note,
        } => {
            let mut params = json!({ "task_id": task_id, "note": note, "from": owner_from_env() });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            maybe_inject_caller_pane(&mut params);
            call_plan_rpc(client, "plan.qa.pass", params).map(Some)
        }
        PlanQaAction::Fail {
            plan_id,
            task_id,
            note,
        } => {
            let mut params = json!({ "task_id": task_id, "note": note, "from": owner_from_env() });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            maybe_inject_caller_pane(&mut params);
            call_plan_rpc(client, "plan.qa.fail", params).map(Some)
        }
    }
}

fn qa_entry_line(entry: &Value) -> String {
    let task_id = entry.get("task_id").and_then(|v| v.as_str()).unwrap_or("?");
    let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let state = entry
        .get("qa_state")
        .and_then(|v| v.as_str())
        .unwrap_or("queued");
    let pos = entry
        .get("position")
        .and_then(|v| v.as_u64())
        .map(|n| format!("#{n} "))
        .unwrap_or_default();
    format!("- {pos}{task_id} [{state}] {name}\n")
}

fn render_qa_queue(value: &Value) -> String {
    let mut out = String::new();
    let running = value
        .get("running")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let queued = value
        .get("queued")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    out.push_str(&format!("Running QA: {}\n", running.len()));
    for entry in &running {
        out.push_str(&qa_entry_line(entry));
    }
    out.push_str(&format!("Queued QA: {}\n", queued.len()));
    for entry in &queued {
        out.push_str(&qa_entry_line(entry));
    }
    out
}

fn render_qa_claim(value: &Value) -> String {
    let Some(claimed) = value.get("claimed").filter(|v| !v.is_null()) else {
        return "No QA task claimed.\n".into();
    };
    let task_id = claimed
        .get("task_id")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    let name = claimed
        .get("task_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let mut out = format!("Claimed QA task {task_id}: {name}\n");
    let test_plan = claimed
        .get("test_plan")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(test_plan) = test_plan {
        if crate::cmd::test_plan::test_plan_is_vague(test_plan) {
            out.push('\n');
            out.push_str(crate::cmd::test_plan::TEST_PLAN_VAGUE_BANNER);
            out.push('\n');
        }
        out.push_str("\nTest plan:\n");
        out.push_str(test_plan);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_prepends_banner_for_vague_test_plan() {
        let value = json!({
            "claimed": {
                "task_id": "task_x",
                "task_name": "Vague one",
                "test_plan": "component tests; manual check"
            }
        });
        let out = render_qa_claim(&value);
        assert!(out.contains(crate::cmd::test_plan::TEST_PLAN_VAGUE_BANNER));
    }

    #[test]
    fn claim_has_no_banner_for_executable_test_plan() {
        let value = json!({
            "claimed": {
                "task_id": "task_y",
                "task_name": "Concrete one",
                "test_plan": "Run `cargo test test_plan_vague` and assert exactly one test_plan_vague finding \
on the seeded plan; then `codeterm plan doctor <plan> --kind quality` round-trips clean."
            }
        });
        let out = render_qa_claim(&value);
        assert!(!out.contains(crate::cmd::test_plan::TEST_PLAN_VAGUE_BANNER));
        assert!(out.contains("Test plan:"));
    }
}
