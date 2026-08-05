use serde_json::Value;

use crate::multiplexer::commands::MuxState;
use crate::service::{ServiceState, TeamPlanError};

use super::plan::plan_rpc_err;
use super::{get_string_param, opt_string_param, JsonRpcError};

pub(super) fn team_plan_rpc_err(error: TeamPlanError) -> JsonRpcError {
    match error {
        TeamPlanError::Team(message) => JsonRpcError {
            code: -32602,
            message,
            data: None,
        },
        TeamPlanError::Plan(error) => plan_rpc_err(error),
    }
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    mux: &MuxState,
    svc: &ServiceState,
) -> Result<Value, JsonRpcError> {
    match method {
        "team.list" => Ok(serde_json::json!({ "teams": mux.read().list_teams() })),
        "team.show" => {
            let team_id = get_string_param(params, "team_id")?;
            let team = mux.read().get_team(&team_id).cloned().ok_or(JsonRpcError {
                code: -32602,
                message: format!("Team {team_id} not found"),
                data: None,
            })?;
            Ok(serde_json::json!({ "team": team }))
        }
        "team.rename" => {
            let team_id = get_string_param(params, "team_id")?;
            let name = get_string_param(params, "name")?;
            let caller_tab_id = opt_string_param(params, "caller_tab_id");
            let team = svc
                .rename_team(
                    &team_id,
                    name,
                    crate::service::TeamMutationCaller::from_tab(caller_tab_id.as_deref()),
                )
                .map_err(|message| JsonRpcError {
                    code: -32602,
                    message,
                    data: None,
                })?;
            Ok(serde_json::json!({ "team": team }))
        }
        "team.close" => {
            let team_id = get_string_param(params, "team_id")?;
            let caller_tab_id = opt_string_param(params, "caller_tab_id");
            let team = svc
                .close_team(
                    &team_id,
                    crate::service::TeamMutationCaller::from_tab(caller_tab_id.as_deref()),
                )
                .map_err(|message| JsonRpcError {
                    code: -32602,
                    message,
                    data: None,
                })?;
            Ok(serde_json::json!({ "team": team }))
        }
        "team.adopt_plan" => {
            let team_id = get_string_param(params, "team_id")?;
            let plan_id = get_string_param(params, "plan_id")?;
            let caller_tab_id = adopt_plan_caller(params)?;
            let plan = svc
                .adopt_plan_for_team(
                    &team_id,
                    &plan_id,
                    crate::service::TeamMutationCaller::AgentTab(caller_tab_id),
                    super::plan_time::epoch_ms(),
                )
                .map_err(team_plan_rpc_err)?;
            Ok(serde_json::json!({ "team_id": team_id, "plan": plan }))
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".into(),
            data: None,
        }),
    }
}

fn adopt_plan_caller(params: &Option<Value>) -> Result<String, JsonRpcError> {
    params
        .as_ref()
        .and_then(|value| value.get("caller_tab_id"))
        .and_then(Value::as_str)
        .filter(|tab_id| !tab_id.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| JsonRpcError {
            code: -32602,
            message: "caller_tab_id is required for Team plan adoption".into(),
            data: None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn adopt_plan_caller_requires_nonempty_tab_id() {
        assert_eq!(
            adopt_plan_caller(&Some(json!({ "caller_tab_id": "manager" }))).unwrap(),
            "manager"
        );
        for params in [
            None,
            Some(json!({})),
            Some(json!({ "caller_tab_id": null })),
            Some(json!({ "caller_tab_id": "  " })),
        ] {
            let error = adopt_plan_caller(&params).unwrap_err();
            assert_eq!(error.code, -32602);
            assert!(error.message.contains("caller_tab_id is required"));
        }
    }
}
