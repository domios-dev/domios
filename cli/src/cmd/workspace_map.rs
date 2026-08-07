use std::collections::{HashMap, HashSet};

use serde_json::Value;

#[derive(Clone)]
struct TabRow {
    id: String,
    label: String,
    workspace_id: String,
    team_id: String,
    parent_id: Option<String>,
    model: String,
    role: String,
    status: String,
    is_self: bool,
}

#[derive(Clone)]
struct TeamRow {
    id: String,
    name: String,
    workspace_id: String,
    lifecycle: String,
}

pub(super) fn render(
    workspaces: &Value,
    tabs: &Value,
    teams: &Value,
    plans: &Value,
    caller_workspace_id: Option<&str>,
    caller_tab_id: Option<&str>,
) -> String {
    let workspace_rows = workspaces.as_array().map(Vec::as_slice).unwrap_or(&[]);
    let tabs_available = tabs.is_array();
    let tabs = tab_rows(tabs, caller_tab_id);
    let teams = team_rows(teams);
    let plans = active_plans(plans);
    let mut rendered_teams = HashSet::new();
    let mut out = String::new();

    for (index, workspace) in workspace_rows.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let workspace_id = text(workspace, "id");
        let workspace_name = text(workspace, "name");
        let mut markers = Vec::new();
        if caller_workspace_id == Some(workspace_id) {
            markers.push("self");
        }
        if workspace.get("is_active").and_then(Value::as_bool) == Some(true)
            && caller_workspace_id != Some(workspace_id)
        {
            markers.push("focused");
        }
        let marker = if markers.is_empty() {
            String::new()
        } else {
            format!("  ({})", markers.join(", "))
        };
        out.push_str(&format!("{}  {}{}\n", workspace_name, workspace_id, marker));

        let ordered_ids = workspace
            .get("tab_ids")
            .and_then(Value::as_array)
            .map(|ids| ids.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut workspace_tabs = ordered_tabs(&tabs, &ordered_ids, workspace_id);
        let mut team_ids = Vec::new();
        for tab in &workspace_tabs {
            if !tab.team_id.is_empty() && !team_ids.contains(&tab.team_id) {
                team_ids.push(tab.team_id.clone());
            }
        }
        for team in &teams {
            if team.workspace_id == workspace_id
                && !team_ids.contains(&team.id)
                && !teams_have_tabs(&tabs, &team.id)
            {
                team_ids.push(team.id.clone());
            }
        }

        for team_id in team_ids {
            rendered_teams.insert(team_id.clone());
            let team = teams.iter().find(|team| team.id == team_id);
            render_team_header(&mut out, team, &team_id, plans.get(&team_id), 1);
            let team_tabs = workspace_tabs
                .iter()
                .filter(|tab| tab.team_id == team_id)
                .cloned()
                .collect::<Vec<_>>();
            if team_tabs.is_empty() {
                out.push_str(if tabs_available {
                    "    (no tabs)\n"
                } else {
                    "    (tabs unavailable)\n"
                });
            } else {
                render_tab_tree(&mut out, &team_tabs, 2);
            }
        }

        workspace_tabs.retain(|tab| tab.team_id.is_empty());
        if !workspace_tabs.is_empty() {
            render_tab_tree(&mut out, &workspace_tabs, 1);
        } else if !tabs.iter().any(|tab| tab.workspace_id == workspace_id)
            && !teams
                .iter()
                .any(|team| team.workspace_id == workspace_id && rendered_teams.contains(&team.id))
        {
            out.push_str(if tabs_available {
                "  (no tabs)\n"
            } else {
                "  (tabs unavailable)\n"
            });
        }
    }

    let orphaned = teams
        .iter()
        .filter(|team| !rendered_teams.contains(&team.id))
        .collect::<Vec<_>>();
    if !orphaned.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("(missing workspace)\n");
        for team in orphaned {
            render_team_header(&mut out, Some(team), &team.id, plans.get(&team.id), 1);
        }
    }

    if out.is_empty() {
        out.push_str("(no workspaces)\n");
    }
    out
}

fn tab_rows(value: &Value, caller_tab_id: Option<&str>) -> Vec<TabRow> {
    value
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter_map(|tab| {
            let id = text(tab, "id");
            if id.is_empty() {
                return None;
            }
            let label = ["tab_title", "title"]
                .into_iter()
                .map(|key| text(tab, key))
                .find(|value| !value.is_empty())
                .unwrap_or(id);
            let model = ["agent_session_model", "agent_type"]
                .into_iter()
                .map(|key| text(tab, key))
                .find(|value| !value.is_empty())
                .unwrap_or_default();
            let role = ["role_profile", "role"]
                .into_iter()
                .map(|key| text(tab, key))
                .find(|value| !value.is_empty())
                .unwrap_or_default();
            Some(TabRow {
                id: id.to_string(),
                label: label.to_string(),
                workspace_id: text(tab, "workspace_id").to_string(),
                team_id: text(tab, "team_id").to_string(),
                parent_id: tab
                    .get("parent_tab_id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
                model: model.to_string(),
                role: role.to_ascii_lowercase(),
                status: text(tab, "status").to_ascii_lowercase(),
                is_self: caller_tab_id == Some(id),
            })
        })
        .collect()
}

fn team_rows(value: &Value) -> Vec<TeamRow> {
    value
        .get("teams")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter_map(|team| {
            let id = text(team, "id");
            (!id.is_empty()).then(|| TeamRow {
                id: id.to_string(),
                name: text(team, "name").to_string(),
                workspace_id: text(team, "workspace_id").to_string(),
                lifecycle: text(team, "lifecycle").to_string(),
            })
        })
        .collect()
}

fn active_plans(value: &Value) -> HashMap<String, String> {
    value
        .get("plans")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .filter(|plan| text(plan, "status") == "active")
        .filter_map(|plan| {
            let team_id = text(plan, "team_id");
            let plan_id = text(plan, "plan_id");
            (!team_id.is_empty() && !plan_id.is_empty())
                .then(|| (team_id.to_string(), plan_id.to_string()))
        })
        .collect()
}

fn ordered_tabs(tabs: &[TabRow], ids: &[&str], workspace_id: &str) -> Vec<TabRow> {
    let by_id = tabs
        .iter()
        .map(|tab| (tab.id.as_str(), tab))
        .collect::<HashMap<_, _>>();
    let mut out = ids
        .iter()
        .filter_map(|id| by_id.get(id).map(|tab| (*tab).clone()))
        .collect::<Vec<_>>();
    out.extend(
        tabs.iter()
            .filter(|tab| tab.workspace_id == workspace_id && !ids.contains(&tab.id.as_str()))
            .cloned(),
    );
    out
}

fn teams_have_tabs(tabs: &[TabRow], team_id: &str) -> bool {
    tabs.iter().any(|tab| tab.team_id == team_id)
}

fn render_team_header(
    out: &mut String,
    team: Option<&TeamRow>,
    team_id: &str,
    plan_id: Option<&String>,
    depth: usize,
) {
    let name = team.map(|team| team.name.as_str()).unwrap_or_default();
    let lifecycle = team.map(|team| team.lifecycle.as_str()).unwrap_or_default();
    let name = if name.is_empty() {
        String::new()
    } else {
        format!(" {name}")
    };
    let plan = plan_id.map(|id| format!("  plan={id}")).unwrap_or_default();
    let state = if lifecycle.is_empty() || lifecycle == "active" {
        String::new()
    } else {
        format!("  [{lifecycle}]")
    };
    out.push_str(&format!(
        "{}team{}  {}{}{}\n",
        "  ".repeat(depth),
        name,
        team_id,
        plan,
        state
    ));
}

fn render_tab_tree(out: &mut String, tabs: &[TabRow], base_depth: usize) {
    let ids = tabs
        .iter()
        .map(|tab| tab.id.as_str())
        .collect::<HashSet<_>>();
    let mut children: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut roots = Vec::new();
    for (index, tab) in tabs.iter().enumerate() {
        match tab.parent_id.as_deref() {
            Some(parent) if ids.contains(parent) => children.entry(parent).or_default().push(index),
            _ => roots.push(index),
        }
    }
    let mut stack = roots
        .into_iter()
        .rev()
        .map(|index| (index, base_depth))
        .collect::<Vec<_>>();
    while let Some((index, depth)) = stack.pop() {
        let tab = &tabs[index];
        let details = [tab.model.as_str(), tab.role.as_str(), tab.status.as_str()]
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        let details = if details.is_empty() {
            String::new()
        } else {
            format!("  [{}]", details.join(" · "))
        };
        let current = if tab.is_self { "  (self)" } else { "" };
        out.push_str(&format!(
            "{}{}  {}{}{}\n",
            "  ".repeat(depth),
            tab.id,
            tab.label,
            details,
            current
        ));
        if let Some(nested) = children.get(tab.id.as_str()) {
            for child in nested.iter().rev() {
                stack.push((*child, depth + 1));
            }
        }
    }
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_workspace_team_plan_tabs_and_empty_workspace() {
        let workspaces = json!([
            {"id":"ws_code","name":"codeterm","is_active":false,"tab_ids":["orch","worker","shell"]},
            {"id":"ws_empty","name":"empty","is_active":true,"tab_ids":[]}
        ]);
        let tabs = json!([
            {"id":"worker","tab_title":"Docs audit","workspace_id":"ws_code","team_id":"team_cli","parent_tab_id":"orch","agent_session_model":"sonnet","role_profile":"worker","status":"working"},
            {"id":"orch","tab_title":"CLI audit","workspace_id":"ws_code","team_id":"team_cli","agent_session_model":"opus","role_profile":"orchestrator","status":"idle"},
            {"id":"shell","tab_title":"logs","workspace_id":"ws_code"}
        ]);
        let teams = json!({"teams":[{"id":"team_cli","name":"CLI council","workspace_id":"ws_code","lifecycle":"active"}]});
        let plans = json!({"plans":[{"plan_id":"pl_cli","team_id":"team_cli","status":"active"}]});

        let output = render(
            &workspaces,
            &tabs,
            &teams,
            &plans,
            Some("ws_code"),
            Some("orch"),
        );

        assert_eq!(
            output,
            "codeterm  ws_code  (self)\n  team CLI council  team_cli  plan=pl_cli\n    orch  CLI audit  [opus · orchestrator · idle]  (self)\n      worker  Docs audit  [sonnet · worker · working]\n  shell  logs\n\nempty  ws_empty  (focused)\n  (no tabs)\n"
        );
    }

    #[test]
    fn places_live_team_with_its_tabs_instead_of_stale_registered_workspace() {
        let workspaces = json!([{"id":"ws_live","name":"live","tab_ids":["orch"]}]);
        let tabs =
            json!([{"id":"orch","title":"audit","workspace_id":"ws_live","team_id":"team_cli"}]);
        let teams =
            json!({"teams":[{"id":"team_cli","workspace_id":"ws_deleted","lifecycle":"forming"}]});

        let output = render(&workspaces, &tabs, &teams, &json!({"plans":[]}), None, None);

        assert!(output.contains("live  ws_live\n  team  team_cli  [forming]\n    orch  audit\n"));
        assert!(!output.contains("(missing workspace)"));
    }

    #[test]
    fn exposes_team_whose_workspace_and_tabs_are_gone() {
        let output = render(
            &json!([]),
            &json!([]),
            &json!({"teams":[{"id":"team_lost","workspace_id":"ws_gone","lifecycle":"forming"}]}),
            &json!({"plans":[]}),
            None,
            None,
        );

        assert_eq!(
            output,
            "(missing workspace)\n  team  team_lost  [forming]\n"
        );
    }

    #[test]
    fn unavailable_tab_data_is_not_rendered_as_an_empty_workspace() {
        let output = render(
            &json!([{"id":"ws_one","name":"one","tab_ids":[]}]),
            &Value::Null,
            &json!({"teams":[]}),
            &json!({"plans":[]}),
            None,
            None,
        );

        assert_eq!(output, "one  ws_one\n  (tabs unavailable)\n");
    }
}
