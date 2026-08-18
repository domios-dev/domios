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
            let mux = mux.read();
            let team = mux.get_team(&team_id).cloned().ok_or(JsonRpcError {
                code: -32602,
                message: format!("Team {team_id} not found"),
                data: None,
            })?;
            Ok(serde_json::json!({ "team": team, "members": team_members(&mux, &team_id) }))
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
        "team.adopt_agent" => {
            let team_id = get_string_param(params, "team_id")?;
            let tab_id = get_string_param(params, "tab_id")?;
            let caller_tab_id = required_caller_tab(params, "Team agent adoption")?;
            let team = svc
                .adopt_agent_for_team(&team_id, &tab_id, &caller_tab_id)
                .map_err(|message| JsonRpcError {
                    code: -32602,
                    message,
                    data: None,
                })?;
            Ok(serde_json::json!({ "team_id": team_id, "tab_id": tab_id, "team": team }))
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".into(),
            data: None,
        }),
    }
}

/// Member tabs of a Team, ordered by tab id so the listing is stable.
fn team_members(mux: &crate::multiplexer::state::MultiplexerState, team_id: &str) -> Value {
    let mut members: Vec<Value> = mux
        .snapshot()
        .tabs
        .values()
        .filter(|tab| tab.team_id.as_deref() == Some(team_id))
        .map(|tab| {
            serde_json::json!({
                "tab_id": tab.id,
                "title": tab.title,
                "role": tab.role_kind_id().unwrap_or("roleless"),
                "anchors_team": tab.anchors_team(),
            })
        })
        .collect();
    members.sort_by(|a, b| a["tab_id"].as_str().cmp(&b["tab_id"].as_str()));
    Value::Array(members)
}

fn adopt_plan_caller(params: &Option<Value>) -> Result<String, JsonRpcError> {
    required_caller_tab(params, "Team plan adoption")
}

/// A Team mutation that authorizes against membership needs the caller's own
/// tab; an absent caller is a refusal, never an implicit direct-user path.
fn required_caller_tab(params: &Option<Value>, action: &str) -> Result<String, JsonRpcError> {
    params
        .as_ref()
        .and_then(|value| value.get("caller_tab_id"))
        .and_then(Value::as_str)
        .filter(|tab_id| !tab_id.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| JsonRpcError {
            code: -32602,
            message: format!("caller_tab_id is required for {action}"),
            data: None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn required_caller_tab_names_the_refused_action() {
        let error = required_caller_tab(&Some(json!({})), "Team agent adoption").unwrap_err();
        assert_eq!(error.code, -32602);
        assert!(
            error.message.contains("Team agent adoption"),
            "{}",
            error.message
        );
    }

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
