use crate::args::plan::PlanReviewAction;
use crate::client::RpcClient;
use serde_json::{json, Value};

use super::{call_plan_rpc, inject_trusted_session, maybe_inject_caller_pane, owner_from_env};

pub(super) fn handle(
    client: &mut dyn RpcClient,
    json_output: bool,
    action: PlanReviewAction,
) -> Result<Option<Value>, String> {
    match action {
        PlanReviewAction::Status { plan_id } => {
            let mut params = json!({});
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            let val = call_plan_rpc(client, "plan.review.status", params)?;
            if json_output {
                return Ok(Some(val));
            }
            print!("{}", render_status(&val));
            Ok(None)
        }
        PlanReviewAction::Waves { plan_id } => {
            let mut params = json!({});
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            let val = call_plan_rpc(client, "plan.review.waves", params)?;
            if json_output {
                return Ok(Some(val));
            }
            print!("{}", render_waves(&val));
            Ok(None)
        }
        PlanReviewAction::Override {
            plan_id,
            task_id,
            owner,
            reason,
            reproduction,
        } => {
            let mut params = json!({
                "task_id": task_id,
                "owner": owner,
                "reason": reason,
                "reproduction": reproduction,
                "from": owner_from_env(),
            });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            maybe_inject_caller_pane(&mut params);
            call_plan_rpc(client, "plan.review.override", params).map(Some)
        }
        PlanReviewAction::ReconcileCredit { plan_id, reason } => {
            let mut params = json!({
                "reason": reason,
            });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            inject_trusted_session(&mut params);
            call_plan_rpc(client, "plan.review.reconcile-credit", params).map(Some)
        }
        PlanReviewAction::AuthorizeCapacity {
            plan_id,
            task_id,
            class,
        } => {
            let mut params = json!({
                "task_id": task_id,
                "class": class,
            });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            inject_trusted_session(&mut params);
            call_plan_rpc(client, "plan.review.authorize-capacity", params).map(Some)
        }
        PlanReviewAction::Accept {
            plan_id,
            task_id,
            note,
            changed_executable_surfaces,
            distinct_oracle,
            residual_risk,
        } => {
            let mut params = json!({
                "task_id": task_id,
                "note": note,
                "changed_executable_surfaces": changed_executable_surfaces,
                "distinct_oracle": distinct_oracle,
                "from": owner_from_env(),
            });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            if let Some(residual_risk) = residual_risk {
                params["residual_risk"] = json!(residual_risk);
            }
            maybe_inject_caller_pane(&mut params);
            call_plan_rpc(client, "plan.review.accept", params).map(Some)
        }
        PlanReviewAction::Bounce {
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
                "from": owner_from_env(),
            });
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            maybe_inject_caller_pane(&mut params);
            call_plan_rpc(client, "plan.review.bounce", params).map(Some)
        }
    }
}

fn render_status(value: &Value) -> String {
    let Some(gate) = value.get("review_gate") else {
        return "No review gate status available.\n".into();
    };
    let wip_count = gate.get("wip_count").and_then(|v| v.as_u64()).unwrap_or(0);
    let wip_limit = gate.get("wip_limit").and_then(|v| v.as_u64()).unwrap_or(0);
    let credit = gate.get("credit").and_then(|v| v.as_i64()).unwrap_or(0);
    let blocked = gate
        .get("blocked")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut out =
        format!("Review WIP: {wip_count}/{wip_limit}  credit: {credit}  blocked: {blocked}\n");
    if let Some(reason) = gate.get("reason").and_then(|v| v.as_str()) {
        out.push_str(&format!("Reason: {reason}\n"));
    }
    out
}

fn render_waves(value: &Value) -> String {
    let waves = value
        .get("waves")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if waves.is_empty() {
        return "No QA waves pending.\n".into();
    }
    let mut out = String::new();
    for wave in &waves {
        let key = wave
            .get("group_key")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let label = wave.get("label").and_then(|v| v.as_str()).unwrap_or(key);
        let count = wave
            .get("task_ids")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        out.push_str(&format!("- {key} ({label}): {count} task(s)\n"));
    }
    out
}
