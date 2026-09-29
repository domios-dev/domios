use crate::client::RpcClient;

const WORKSPACES_PATH: &str = "/api/workspaces";
const ALL_LOCAL_TABS_PATH: &str = "/api/tabs?local_only=true";

pub(super) fn should_render(command: &crate::Commands, json: bool) -> bool {
    !json
        && matches!(
            command,
            crate::Commands::Workspace {
                action: crate::WorkspaceAction::List
            }
        )
}

struct WorkspaceTopologyData {
    workspaces: serde_json::Value,
    tabs: Option<serde_json::Value>,
    teams: Option<serde_json::Value>,
    plans: Option<serde_json::Value>,
}

fn fetch_workspace_topology<Rest, Rpc>(
    rest: &mut Rest,
    rpc: &mut Rpc,
) -> Result<(WorkspaceTopologyData, Vec<String>), String>
where
    Rest: FnMut(&str, &str) -> Result<serde_json::Value, String>,
    Rpc: FnMut(&str, Option<serde_json::Value>) -> Result<serde_json::Value, String>,
{
    let workspaces = rest("GET", WORKSPACES_PATH)?;
    let mut warnings = Vec::new();
    let tabs = optional_source(
        rest("GET", ALL_LOCAL_TABS_PATH),
        "remote Tabs",
        &mut warnings,
    );
    let teams = optional_source(rpc("team.list", None), "remote Teams", &mut warnings);
    let plans = optional_source(
        rpc("plan.list", Some(serde_json::json!({ "status": "active" }))),
        "remote Plans",
        &mut warnings,
    );
    Ok((
        WorkspaceTopologyData {
            workspaces,
            tabs,
            teams,
            plans,
        },
        warnings,
    ))
}

fn optional_source(
    result: Result<serde_json::Value, String>,
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<serde_json::Value> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            warnings.push(format!("warning: unable to read {label}: {error}"));
            None
        }
    }
}

pub(super) fn print_remote_workspace_topology(target: &str) {
    let (base, token) = super::super::read_api_config();
    let mut rpc_client = match super::rpc::MeshRpcClient::connect(&base, &token, target) {
        Ok(client) => client,
        Err(error) => {
            crate::cmd::print::fail(1, error.to_string());
        }
    };
    let mut rest = |method: &str, path: &str| {
        super::relay::mesh_relay_invoke_result(target, method, path, None)
            .map_err(|failure| failure.to_string())
    };
    let mut rpc = |method: &str, params: Option<serde_json::Value>| rpc_client.call(method, params);
    let (data, warnings) = match fetch_workspace_topology(&mut rest, &mut rpc) {
        Ok(result) => result,
        Err(error) => {
            crate::cmd::print::fail(1, format!("mesh request to '{target}' failed: {error}"));
        }
    };
    for warning in warnings {
        eprintln!("{warning}");
    }
    print!(
        "{}",
        crate::cmd::render_workspace_map(
            &data.workspaces,
            data.tabs.as_ref(),
            data.teams.as_ref(),
            data.plans.as_ref(),
            None,
            None,
        )
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_workspace_list_bypasses_topology_composition() {
        let command = crate::Commands::Workspace {
            action: crate::WorkspaceAction::List,
        };

        assert!(!should_render(&command, true));
        assert!(should_render(&command, false));
    }

    #[test]
    fn full_topology_fetch_uses_remote_rest_and_rpc_sources() {
        let mut rest_calls = Vec::new();
        let mut rpc_calls = Vec::new();
        let mut rest = |method: &str, path: &str| {
            rest_calls.push((method.to_string(), path.to_string()));
            Ok(match path {
                WORKSPACES_PATH => json!([{"id":"ws_remote","tab_ids":["tab_remote"]}]),
                ALL_LOCAL_TABS_PATH => {
                    json!([{
                        "id":"tab_remote",
                        "workspace_id":"ws_remote",
                        "team_id":"team_remote"
                    }])
                }
                _ => unreachable!(),
            })
        };
        let mut rpc = |method: &str, params: Option<serde_json::Value>| {
            rpc_calls.push((method.to_string(), params));
            Ok(match method {
                "team.list" => json!({"teams":[{
                    "id":"team_remote",
                    "workspace_id":"ws_remote"
                }]}),
                "plan.list" => json!({"plans":[{
                    "plan_id":"pl_remote",
                    "team_id":"team_remote",
                    "status":"active"
                }]}),
                _ => unreachable!(),
            })
        };

        let (data, warnings) = fetch_workspace_topology(&mut rest, &mut rpc).unwrap();

        assert_eq!(
            rest_calls,
            [
                ("GET".to_string(), WORKSPACES_PATH.to_string()),
                ("GET".to_string(), ALL_LOCAL_TABS_PATH.to_string())
            ]
        );
        assert_eq!(rpc_calls[0], ("team.list".to_string(), None));
        assert_eq!(rpc_calls[1].0, "plan.list");
        assert_eq!(rpc_calls[1].1.as_ref().unwrap()["status"], "active");
        assert_eq!(data.workspaces[0]["id"], "ws_remote");
        assert_eq!(data.tabs.as_ref().unwrap()[0]["id"], "tab_remote");
        assert!(warnings.is_empty());
        let rendered = crate::cmd::render_workspace_map(
            &data.workspaces,
            data.tabs.as_ref(),
            data.teams.as_ref(),
            data.plans.as_ref(),
            None,
            None,
        );
        for expected in ["ws_remote", "tab_remote", "team_remote", "pl_remote"] {
            assert!(
                rendered.contains(expected),
                "missing {expected}: {rendered}"
            );
        }
    }

    #[test]
    fn workspace_spine_failure_stops_auxiliary_reads() {
        let mut rest_calls = 0;
        let mut rpc_calls = 0;
        let mut rest = |_method: &str, _path: &str| {
            rest_calls += 1;
            Err("peer unavailable".to_string())
        };
        let mut rpc = |_method: &str, _params: Option<serde_json::Value>| {
            rpc_calls += 1;
            Ok(serde_json::Value::Null)
        };

        let result = fetch_workspace_topology(&mut rest, &mut rpc);

        assert!(result.is_err());
        assert_eq!(rest_calls, 1);
        assert_eq!(rpc_calls, 0);
    }

    #[test]
    fn auxiliary_failures_keep_the_workspace_spine() {
        let mut rest = |_method: &str, path: &str| match path {
            WORKSPACES_PATH => Ok(json!([{"id":"ws_remote","name":"remote","tab_ids":[]}])),
            ALL_LOCAL_TABS_PATH => Err("tabs unavailable".to_string()),
            _ => unreachable!(),
        };
        let mut rpc = |method: &str, _params: Option<serde_json::Value>| match method {
            "team.list" => Err("teams unavailable".to_string()),
            "plan.list" => Err("plans unavailable".to_string()),
            _ => unreachable!(),
        };

        let (data, warnings) = fetch_workspace_topology(&mut rest, &mut rpc).unwrap();
        let rendered = crate::cmd::render_workspace_map(
            &data.workspaces,
            data.tabs.as_ref(),
            data.teams.as_ref(),
            data.plans.as_ref(),
            None,
            None,
        );

        assert!(rendered.contains("ws_remote"));
        assert!(rendered.contains("tabs unavailable"));
        assert_eq!(warnings.len(), 3);
    }

    #[test]
    fn remote_topology_marks_active_without_claiming_local_caller_identity() {
        let rendered = crate::cmd::render_workspace_map(
            &json!([{
                "id":"ws_remote",
                "name":"remote",
                "is_active":true,
                "tab_ids":["tab_remote"]
            }]),
            Some(&json!([{"id":"tab_remote","title":"Peer tab","workspace_id":"ws_remote"}])),
            Some(&json!({"teams":[]})),
            Some(&json!({"plans":[]})),
            None,
            None,
        );

        assert!(rendered.contains("(active)"));
        assert!(!rendered.contains("(self)"));
    }
}
