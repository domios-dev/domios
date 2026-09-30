use serde_json::{json, Value};

use crate::client::RpcClient;

pub(crate) fn validate_plan_delete_confirmation(plan_id: &str, yes: bool) -> Result<(), String> {
    if !yes {
        return Err(format!(
            "refusing to delete plan '{plan_id}' without --yes; pass --yes or --confirm to permanently delete it"
        ));
    }
    Ok(())
}

pub(super) fn plan_mutation_params(
    plan_id: String,
    caller_tab_id: Option<&str>,
    human_confirmation: bool,
) -> Value {
    let mut params = json!({ "plan_id": plan_id });
    if let Some(tab_id) = caller_tab_id.filter(|tab_id| !tab_id.trim().is_empty()) {
        params["caller_tab_id"] = json!(tab_id);
    } else if human_confirmation {
        params["human_confirmation"] = json!(true);
    }
    params
}

pub(super) fn plan_archive_params(
    plan_id: String,
    yes: bool,
    caller_tab_id: Option<&str>,
) -> Result<Value, String> {
    if caller_tab_id.is_none_or(|tab_id| tab_id.trim().is_empty()) && !yes {
        return Err(format!(
            "refusing to archive plan '{plan_id}' without a Plan tab identity or --yes"
        ));
    }
    Ok(plan_mutation_params(plan_id, caller_tab_id, yes))
}

pub(super) fn plan_delete_params(
    plan_id: String,
    yes: bool,
    caller_tab_id: Option<&str>,
) -> Result<Value, String> {
    validate_plan_delete_confirmation(&plan_id, yes)?;
    Ok(plan_mutation_params(plan_id, caller_tab_id, yes))
}

pub(super) fn plan_transfer_params(
    plan_id: String,
    to: String,
    reason: Option<String>,
    caller_tab_id: Option<&str>,
) -> Result<Value, String> {
    let caller_tab_id = caller_tab_id
        .filter(|tab_id| !tab_id.trim().is_empty())
        .ok_or_else(|| {
            "plan transfer requires CODETERM_TAB_ID from the calling Team manager".to_string()
        })?;
    let mut params = json!({
        "plan_id": plan_id,
        "to": { "tab_id": to },
        "caller_tab_id": caller_tab_id,
    });
    if let Some(reason) = reason {
        params["reason"] = json!(reason);
    }
    Ok(params)
}

pub(super) fn default_project_path(client: &dyn RpcClient) -> Option<String> {
    if client.is_remote() {
        return None;
    }
    std::env::current_dir()
        .ok()
        .map(|path| path.display().to_string())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn lane_params(
    tab_id: Option<String>,
    workspace_id: Option<String>,
    workspace_name: Option<String>,
    worktree_path: Option<String>,
    branch: Option<String>,
    base: Option<String>,
    destination: Option<String>,
    cleanup_status: Option<String>,
    cleanup_hints: Vec<String>,
) -> Option<Value> {
    let mut lane = json!({});
    if let Some(value) = tab_id {
        lane["tab_id"] = json!(value);
    }
    if let Some(value) = workspace_id {
        lane["workspace_id"] = json!(value);
    }
    if let Some(value) = workspace_name {
        lane["workspace_name"] = json!(value);
    }
    if let Some(value) = worktree_path {
        lane["worktree_path"] = json!(value);
    }
    if let Some(value) = branch {
        lane["branch"] = json!(value);
    }
    if let Some(value) = base {
        lane["base"] = json!(value);
    }
    if let Some(value) = destination {
        lane["destination"] = json!(value);
    }
    if let Some(value) = cleanup_status {
        lane["cleanup_status"] = json!(value);
    }
    if !cleanup_hints.is_empty() {
        lane["cleanup_hints"] = json!(cleanup_hints);
    }
    if lane.as_object().is_some_and(|items| !items.is_empty()) {
        Some(lane)
    } else {
        None
    }
}
