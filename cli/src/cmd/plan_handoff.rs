use serde_json::{json, Value};

use crate::args::plan::PlanHandoffAction;
use crate::client::RpcClient;

use super::plan::{call_plan_rpc, owner_from_env, resolve_text};

pub fn handle(
    client: &mut dyn RpcClient,
    json_output: bool,
    action: PlanHandoffAction,
) -> Result<Option<Value>, String> {
    match action {
        PlanHandoffAction::Create {
            plan_id,
            mode,
            task,
            goal,
            next_actions,
            foundational_rules,
            open_decisions,
            from_file,
            stdin,
        } => {
            let narrative = if stdin {
                resolve_text(Some("-".into()), None)?
            } else {
                resolve_text(None, from_file)?
            };
            let mut params = json!({
                "plan_id": plan_id,
                "mode": mode,
                "next_actions": next_actions,
                "foundational_rules": foundational_rules,
                "open_decisions": open_decisions,
                "from": owner_from_env(),
            });
            if let Some(task) = task {
                params["task"] = json!(task);
            }
            if let Some(goal) = goal {
                params["goal"] = json!(goal);
            }
            if let Some(narrative) = narrative {
                params["narrative"] = json!(narrative);
            }
            call_plan_rpc(client, "plan.handoff.create", params).map(Some)
        }
        PlanHandoffAction::Show {
            plan_id,
            selector,
            seq,
            latest,
            markdown,
        } => {
            let mut params = json!({ "plan_id": plan_id });
            if !latest {
                if let Some(selector) = seq.or(selector) {
                    params["selector"] = json!(selector);
                }
            }
            let val = call_plan_rpc(client, "plan.handoff.show", params)?;
            if markdown && !json_output {
                print!(
                    "{}",
                    val.get("markdown")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                );
                return Ok(None);
            }
            Ok(Some(val))
        }
        PlanHandoffAction::List { plan_id } => {
            call_plan_rpc(client, "plan.handoff.list", json!({ "plan_id": plan_id })).map(Some)
        }
    }
}
