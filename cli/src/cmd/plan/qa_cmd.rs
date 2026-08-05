use crate::args::plan::PlanQaAction;
use crate::client::RpcClient;
use serde_json::{json, Value};

use super::call_plan_rpc;
use super::caller_context::CallerContext;

pub(super) fn handle(
    client: &mut dyn RpcClient,
    json_output: bool,
    action: PlanQaAction,
) -> Result<Option<Value>, String> {
    let ctx = CallerContext::from_env();
    match action {
        PlanQaAction::Queue {
            plan_id,
            task,
            note,
        } => {
            let mut params = json!({ "from": ctx.owner() });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            if let Some(task) = task {
                params["task_id"] = json!(task);
            }
            if let Some(note) = note {
                params["note"] = json!(note);
            }
            ctx.apply_caller_tab(&mut params);
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
            group,
            note,
        } => {
            let mut params = json!({ "claim": claim, "from": ctx.owner() });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            if let Some(group) = &group {
                params["group"] = json!(group);
            }
            if let Some(note) = note {
                params["note"] = json!(note);
            }
            ctx.apply_caller_tab(&mut params);
            let val = call_plan_rpc(client, "plan.qa.next", params)?;
            if json_output {
                return Ok(Some(val));
            }
            if group.is_some() {
                print!("{}", render_regression_gate(&val));
            } else if claim {
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
            let mut params = json!({ "task_id": task_id, "note": note, "from": ctx.owner() });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            ctx.apply_caller_tab(&mut params);
            call_plan_rpc(client, "plan.qa.pass", params).map(Some)
        }
        PlanQaAction::Fail {
            plan_id,
            task_id,
            note,
            changed_executable_surfaces,
            distinct_oracle,
        } => {
            let mut params = json!({
                "task_id": task_id,
                "note": note,
                "changed_executable_surfaces": changed_executable_surfaces,
                "distinct_oracle": distinct_oracle,
                "from": ctx.owner()
            });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            ctx.apply_caller_tab(&mut params);
            call_plan_rpc(client, "plan.qa.fail", params).map(Some)
        }
        PlanQaAction::Override {
            plan_id,
            gate,
            reason,
        } => {
            let mut params = json!({ "reason": reason, "gates": gate, "from": ctx.owner() });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            ctx.apply_caller_tab(&mut params);
            let val = call_plan_rpc(client, "plan.qa.override", params)?;
            if json_output {
                return Ok(Some(val));
            }
            print!("{}", render_override(&val));
            Ok(None)
        }
    }
}

fn render_override(value: &Value) -> String {
    let Some(record) = value.get("override").filter(|v| !v.is_null()) else {
        return "No regression gate override recorded.\n".into();
    };
    let gates = record
        .get("gates")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|g| g.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let reason = record.get("reason").and_then(|v| v.as_str()).unwrap_or("");
    let mut out = format!("Recorded override for regression gate(s): {gates}\nReason: {reason}\n");
    let remaining = value
        .get("blocking_gates")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|g| g.get("key").and_then(|k| k.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    if remaining.is_empty() {
        out.push_str("Frontier dispatch unblocked.\n");
    } else {
        out.push_str(&format!("Still blocking: {remaining}\n"));
    }
    out
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

fn render_regression_gate(value: &Value) -> String {
    let Some(gate) = value.get("regression_gate").filter(|v| !v.is_null()) else {
        return "No regression gate recorded.\n".into();
    };
    let key = gate.get("key").and_then(|v| v.as_str()).unwrap_or("?");
    let label = gate.get("label").and_then(|v| v.as_str()).unwrap_or(key);
    format!("Recorded regression gate for group {key} ({label}).\n")
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
        out.push_str("\nQA plan:\n");
        out.push_str(test_plan);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::SocketCallError;

    #[derive(Default)]
    struct FakeRpc {
        calls: Vec<(String, Option<Value>)>,
    }

    impl RpcClient for FakeRpc {
        fn call_detailed(
            &mut self,
            method: &str,
            params: Option<Value>,
        ) -> Result<Value, SocketCallError> {
            self.calls.push((method.into(), params));
            Ok(json!({ "plan_id": "pl_1", "task_id": "task_1" }))
        }
    }

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
    fn fail_threads_strategy_reset_flags_to_rpc() {
        let mut rpc = FakeRpc::default();
        handle(
            &mut rpc,
            true,
            PlanQaAction::Fail {
                plan_id: Some("pl_1".into()),
                task_id: "task_1".into(),
                note: "new implementation and oracle".into(),
                changed_executable_surfaces: true,
                distinct_oracle: true,
            },
        )
        .unwrap();
        let (method, params) = &rpc.calls[0];
        assert_eq!(method, "plan.qa.fail");
        let params = params.as_ref().unwrap();
        assert_eq!(params["changed_executable_surfaces"], true);
        assert_eq!(params["distinct_oracle"], true);
    }

    #[test]
    fn override_render_names_gates_reason_and_unblock() {
        let value = json!({
            "override": {"gates": ["phase-1", "phase-2"], "reason": "warm QA down; owner sign-off"},
            "blocking_gates": []
        });
        let out = render_override(&value);
        assert!(out.contains("phase-1, phase-2"));
        assert!(out.contains("warm QA down; owner sign-off"));
        assert!(out.contains("Frontier dispatch unblocked."));
    }

    #[test]
    fn override_render_reports_remaining_blockers() {
        let value = json!({
            "override": {"gates": ["phase-1"], "reason": "partial waiver"},
            "blocking_gates": [{"key": "phase-2", "label": "Phase 2"}]
        });
        let out = render_override(&value);
        assert!(out.contains("Still blocking: phase-2"));
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
        assert!(out.contains("QA plan:"));
    }
}
