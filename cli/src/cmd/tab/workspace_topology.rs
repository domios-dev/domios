use crate::client::RpcClient;
use crate::local_api::LocalApiClient;
use crate::{build_local_api, nonempty_env};
use serde_json::Value;

const WORKSPACES_PATH: &str = "/api/workspaces";
const ALL_LOCAL_TABS_PATH: &str = "/api/tabs?local_only=true";

pub(super) fn print_local_workspace_map(
    client: &mut dyn RpcClient,
    agents_only: bool,
) -> Result<(), String> {
    let api = build_local_api().map_err(|error| error.to_string())?;
    let workspaces = fetch_rest(&api, WORKSPACES_PATH)
        .map_err(|error| format!("unable to read workspaces: {error}"))?;
    let mut warnings = Vec::new();
    let tabs = optional_source(
        fetch_rest(&api, &all_tabs_path(agents_only)),
        "Tabs",
        &mut warnings,
    );
    let teams = optional_source(client.call("team.list", None), "Teams", &mut warnings);
    let plans = optional_source(
        client.call("plan.list", Some(serde_json::json!({ "status": "active" }))),
        "Plans",
        &mut warnings,
    );
    for warning in warnings {
        eprintln!("{warning}");
    }

    let caller_tab_id = nonempty_env("CODETERM_TAB_ID");
    let caller_workspace_id = caller_tab_id
        .as_deref()
        .and_then(|tab_id| caller_workspace_id(&workspaces, tabs.as_ref(), tab_id));
    print!(
        "{}",
        crate::cmd::render_workspace_map(
            &workspaces,
            tabs.as_ref(),
            teams.as_ref(),
            plans.as_ref(),
            caller_workspace_id.as_deref(),
            caller_tab_id.as_deref(),
        )
    );
    Ok(())
}

fn fetch_rest(api: &LocalApiClient, path: &str) -> Result<Value, String> {
    api.get(path)
        .send()
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json::<Value>()
        .map_err(|error| error.to_string())
}

fn optional_source(
    result: Result<Value, String>,
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<Value> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            warnings.push(format!("warning: unable to read {label}: {error}"));
            None
        }
    }
}

fn all_tabs_path(agents_only: bool) -> String {
    if agents_only {
        format!("{ALL_LOCAL_TABS_PATH}&agents=true")
    } else {
        ALL_LOCAL_TABS_PATH.to_string()
    }
}

fn caller_workspace_id(
    workspaces: &Value,
    tabs: Option<&Value>,
    caller_tab_id: &str,
) -> Option<String> {
    workspaces
        .as_array()
        .into_iter()
        .flatten()
        .find(|workspace| {
            workspace
                .get("tab_ids")
                .and_then(Value::as_array)
                .is_some_and(|tab_ids| tab_ids.iter().any(|id| id.as_str() == Some(caller_tab_id)))
        })
        .and_then(|workspace| workspace.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            tabs?.as_array()?.iter().find_map(|tab| {
                (tab.get("id").and_then(Value::as_str) == Some(caller_tab_id))
                    .then(|| tab.get("workspace_id"))
                    .flatten()
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn local_tabs_path_preserves_the_all_tabs_source_and_agent_filter() {
        assert_eq!(all_tabs_path(false), "/api/tabs?local_only=true");
        assert_eq!(all_tabs_path(true), "/api/tabs?local_only=true&agents=true");
    }

    #[test]
    fn caller_workspace_comes_from_the_workspace_spine_or_tab_source() {
        let workspaces = json!([{
            "id": "ws_one",
            "tab_ids": ["caller"]
        }]);
        assert_eq!(
            caller_workspace_id(&workspaces, None, "caller").as_deref(),
            Some("ws_one")
        );

        let tabs = json!([{
            "id": "caller",
            "workspace_id": "ws_two"
        }]);
        assert_eq!(
            caller_workspace_id(&json!([]), Some(&tabs), "caller").as_deref(),
            Some("ws_two")
        );
    }

    #[test]
    fn optional_sources_warn_and_degrade_to_no_source() {
        let mut warnings = Vec::new();
        assert!(optional_source(Err("rpc unavailable".into()), "Teams", &mut warnings).is_none());
        assert_eq!(warnings, ["warning: unable to read Teams: rpc unavailable"]);
    }
}
