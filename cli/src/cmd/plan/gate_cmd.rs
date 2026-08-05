use crate::args::plan::PlanDecisionAction;
use crate::client::RpcClient;
use serde_json::{json, Value};

use super::call_plan_rpc;
use super::caller_context::CallerContext;

pub(super) fn handle(
    client: &mut dyn RpcClient,
    json_output: bool,
    action: PlanDecisionAction,
) -> Result<Option<Value>, String> {
    let ctx = CallerContext::from_env();
    match action {
        PlanDecisionAction::Ask {
            plan_id,
            question,
            context,
            blocking_task_ids,
            asked_by_tab,
        } => {
            let mut params = json!({
                "plan_id": plan_id,
                "question": question,
                "context": context,
                "blocking_task_ids": blocking_task_ids,
            });
            if let Some(tab) = asked_by_tab {
                params["asked_by_tab_id"] = json!(tab);
            }
            ctx.apply_caller_tab(&mut params);
            let gate = call_plan_rpc(client, "plan.decision.ask", params)?;
            if json_output {
                return Ok(Some(gate));
            }
            println!("{}", render_gate(&gate));
            Ok(None)
        }

        PlanDecisionAction::Answer {
            plan_id,
            gate_id,
            answer,
            answered_by_tab,
        } => {
            let mut params = json!({
                "plan_id": plan_id,
                "gate_id": gate_id,
                "answer": answer,
            });
            if let Some(tab) = answered_by_tab {
                params["answered_by_tab_id"] = json!(tab);
            }
            ctx.apply_caller_tab(&mut params);
            let gate = call_plan_rpc(client, "plan.decision.answer", params)?;
            if json_output {
                return Ok(Some(gate));
            }
            println!("{}", render_gate(&gate));
            Ok(None)
        }

        PlanDecisionAction::AckRequest {
            plan_id,
            request_id,
        } => {
            let mut params = json!({
                "plan_id": plan_id,
                "request_id": request_id,
            });
            ctx.apply_caller_tab(&mut params);
            let request = call_plan_rpc(client, "plan.decision.ack_request", params)?;
            if json_output {
                return Ok(Some(request));
            }
            println!(
                "Acknowledged Planner request {}.",
                request
                    .get("request_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("?")
            );
            Ok(None)
        }

        PlanDecisionAction::List { plan_id, open } => {
            let params = json!({
                "plan_id": plan_id,
                "open_only": open,
            });
            let gates = call_plan_rpc(client, "plan.decision.list", params)?;
            if json_output {
                return Ok(Some(gates));
            }
            if let Some(arr) = gates.as_array() {
                if arr.is_empty() {
                    println!("No{} decision gates.", if open { " open" } else { "" });
                } else {
                    for gate in arr {
                        println!("{}", render_gate(gate));
                    }
                }
            }
            Ok(None)
        }

        PlanDecisionAction::Escalate {
            plan_id,
            gate_id,
            note,
        } => {
            let mut params = json!({
                "plan_id": plan_id,
                "gate_id": gate_id,
            });
            if let Some(note) = note {
                params["note"] = json!(note);
            }
            let gate = call_plan_rpc(client, "plan.decision.escalate", params)?;
            if json_output {
                return Ok(Some(gate));
            }
            println!("{}", render_gate(&gate));
            Ok(None)
        }
    }
}

fn render_gate(gate: &Value) -> String {
    let id = gate.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    let status = gate
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("open");
    let question = gate.get("question").and_then(|v| v.as_str()).unwrap_or("");
    let blocking = gate
        .get("blocking_task_ids")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let routed = gate
        .get("routed_to_tab_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unrouted");
    let mut out = format!("[{id}] {status} blocking:{blocking} routed:{routed}\n  {question}\n");
    if let Some(ans) = gate.get("answer").filter(|v| !v.is_null()) {
        let text = ans.get("text").and_then(|v| v.as_str()).unwrap_or("");
        out.push_str(&format!("  Answer: {text}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn render_open_gate() {
        let gate = json!({
            "id": "gate_abc",
            "status": "open",
            "question": "Which auth strategy?",
            "blocking_task_ids": ["task_1", "task_2"],
            "routed_to_tab_id": "tab_planner",
        });
        let out = render_gate(&gate);
        assert!(out.contains("[gate_abc]"));
        assert!(out.contains("open"));
        assert!(out.contains("blocking:2"));
        assert!(out.contains("routed:tab_planner"));
        assert!(out.contains("Which auth strategy?"));
        assert!(!out.contains("Answer:"));
    }

    #[test]
    fn render_answered_gate_includes_answer() {
        let gate = json!({
            "id": "gate_xyz",
            "status": "answered",
            "question": "Go left or right?",
            "blocking_task_ids": [],
            "routed_to_tab_id": null,
            "answer": { "text": "Go left.", "by_tab_id": "tab_planner", "at": 1000 },
        });
        let out = render_gate(&gate);
        assert!(out.contains("[gate_xyz]"));
        assert!(out.contains("answered"));
        assert!(out.contains("blocking:0"));
        assert!(out.contains("unrouted"));
        assert!(out.contains("Answer: Go left."));
    }

    #[test]
    fn render_gate_missing_fields_fallback() {
        let out = render_gate(&json!({}));
        assert!(out.contains("[?]"));
        assert!(out.contains("blocking:0"));
        assert!(out.contains("unrouted"));
    }
}
