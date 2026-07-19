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
            let caller_pane_id = opt_string_param(params, "caller_pane_id");
            let team = svc
                .rename_team(&team_id, name, caller_pane_id.as_deref())
                .map_err(|message| JsonRpcError {
                    code: -32602,
                    message,
                    data: None,
                })?;
            Ok(serde_json::json!({ "team": team }))
        }
        "team.close" => {
            let team_id = get_string_param(params, "team_id")?;
            let caller_pane_id = opt_string_param(params, "caller_pane_id");
            let team = svc
                .close_team(&team_id, caller_pane_id.as_deref())
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
            let caller_pane_id = opt_string_param(params, "caller_pane_id");
            let plan = svc
                .adopt_plan_for_team(
                    &team_id,
                    &plan_id,
                    caller_pane_id.as_deref(),
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
