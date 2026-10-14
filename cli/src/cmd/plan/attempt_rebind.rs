use crate::client::RpcClient;
use crate::cmd::print::CliError;
use serde_json::{json, Value};

pub(super) fn run(
    client: &mut dyn RpcClient,
    plan_id: Option<String>,
    task_id: Option<String>,
    attempt_id: Option<String>,
) -> Result<Value, CliError> {
    match (task_id, attempt_id) {
        (Some(task_id), Some(attempt_id)) => {
            let plan_id = plan_id.ok_or_else(|| {
                CliError::new(
                    "targeted plan rebind requires an explicit plan id before --task and --attempt",
                )
            })?;
            let shown =
                super::call_plan_rpc_detailed(client, "task.show", json!({ "task_id": &task_id }))?;
            if shown.get("plan_id").and_then(Value::as_str) != Some(plan_id.as_str()) {
                return Err(CliError::new(
                    "the exact task does not belong to the requested plan",
                ));
            }
            let task = shown.get("task").unwrap_or(&shown);
            let owner_epoch = task
                .get("interruption")
                .and_then(|value| value.get("owner_epoch"))
                .and_then(Value::as_u64)
                .filter(|epoch| *epoch > 0)
                .ok_or_else(|| {
                    CliError::new("task detail did not include a current owner epoch")
                })?;
            let exact_task_id = task
                .get("id")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| CliError::new("task detail did not include the exact task id"))?;
            let mut params = json!({
                "plan_id": &plan_id,
                "task_id": exact_task_id,
                "attempt_id": &attempt_id,
                "owner_epoch": owner_epoch,
            });
            super::inject_trusted_session(&mut params);
            let response = super::call_plan_rpc_detailed(client, "plan.session.rebind", params)?;
            super::targeted_rebind_response(response, &plan_id, exact_task_id, &attempt_id)
                .map_err(CliError::new)
        }
        (None, None) => {
            let mut params = json!({});
            if let Some(plan_id) = plan_id {
                params["plan_id"] = json!(plan_id);
            }
            super::inject_trusted_session(&mut params);
            super::call_plan_rpc_detailed(client, "plan.session.rebind", params)
        }
        _ => Err(CliError::new(
            "targeted plan rebind requires both --task and --attempt",
        )),
    }
}
