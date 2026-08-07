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
    tabs: Option<&Value>,
    teams: Option<&Value>,
    plans: Option<&Value>,
    caller_workspace_id: Option<&str>,
    caller_tab_id: Option<&str>,
) -> String {
    let workspace_rows = workspaces.as_array().map(Vec::as_slice).unwrap_or(&[]);
    let tabs_available = tabs.is_some_and(Value::is_array);
    let teams_available = teams.is_some();
    let plans_available = plans.is_some();
    let tabs = tabs
        .map(|value| tab_rows(value, caller_tab_id))
        .unwrap_or_default();
    let teams = teams.map(team_rows).unwrap_or_default();
    let plans = plans.map(active_plans).unwrap_or_default();
    let mut rendered_teams = HashSet::new();
    let mut out = String::new();

    for (index, workspace) in workspace_rows.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let workspace_id = text(workspace, "id");
        let workspace_name = text(workspace, "name");
        let ordered_ids = workspace
            .get("tab_ids")
            .and_then(Value::as_array)
            .map(|ids| ids.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut workspace_tabs = ordered_tabs(&tabs, &ordered_ids, workspace_id);
        let tab_count = workspace
            .get("tab_count")
            .and_then(Value::as_u64)
            .unwrap_or(ordered_ids.len() as u64);
        let mut markers = Vec::<String>::new();
        if caller_workspace_id == Some(workspace_id) {
            markers.push("self".to_string());
        }
        if workspace.get("is_active").and_then(Value::as_bool) == Some(true) {
            markers.push("active".to_string());
        }
        if workspace.get("protected").and_then(Value::as_bool) == Some(true) {
            markers.push("protected".to_string());
        }
        if !tabs_available {
            markers.push("tabs unavailable".to_string());
        } else if tab_count == 0 {
            markers.push("empty".to_string());
        } else if (workspace_tabs.len() as u64) < tab_count {
            markers.push(format!(
                "{} not shown",
                tab_count.saturating_sub(workspace_tabs.len() as u64)
            ));
        }
        let marker = if markers.is_empty() {
            String::new()
        } else {
            format!("  ({})", markers.join(", "))
        };
        let cwd = text(workspace, "cwd");
        let cwd = if cwd.is_empty() {
            String::new()
        } else {
            format!("  {cwd}")
        };
        let unit = if tab_count == 1 { "tab" } else { "tabs" };
        out.push_str(&format!(
            "{}  {}  {} {}{}{}\n",
            workspace_name, workspace_id, tab_count, unit, cwd, marker
        ));

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

        if !teams_available {
            out.push_str("  (teams unavailable)\n");
        }

        let has_team_groups = !team_ids.is_empty();
        for team_id in team_ids {
            rendered_teams.insert(team_id.clone());
            let team = teams.iter().find(|team| team.id == team_id);
            render_team_header(
                &mut out,
                team,
                &team_id,
                plans.get(&team_id),
                teams_available,
                plans_available,
                1,
            );
            let team_tabs = workspace_tabs
                .iter()
                .filter(|tab| tab.team_id == team_id)
                .cloned()
                .collect::<Vec<_>>();
            if team_tabs.is_empty() {
                out.push_str(if tabs_available {
                    "    (no visible tabs)\n"
                } else {
                    "    (tabs unavailable)\n"
                });
            } else {
                render_tab_tree(&mut out, &team_tabs, 2);
            }
        }

        workspace_tabs.retain(|tab| tab.team_id.is_empty());
        if !workspace_tabs.is_empty() {
            if has_team_groups {
                out.push_str("  (no team)\n");
                render_tab_tree(&mut out, &workspace_tabs, 2);
            } else {
                render_tab_tree(&mut out, &workspace_tabs, 1);
            }
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
            render_team_header(
                &mut out,
                Some(team),
                &team.id,
                plans.get(&team.id),
                teams_available,
                plans_available,
                1,
            );
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
    teams_available: bool,
    plans_available: bool,
    depth: usize,
) {
    let name = team.map(|team| team.name.as_str()).unwrap_or_default();
    let lifecycle = team.map(|team| team.lifecycle.as_str()).unwrap_or_default();
    let name = if name.is_empty() {
        String::new()
    } else {
        format!(" {name}")
    };
    let plan = if !plans_available {
        "  plan=?".to_string()
    } else {
        plan_id.map(|id| format!("  plan={id}")).unwrap_or_default()
    };
    let state = if lifecycle.is_empty() || lifecycle == "active" {
        String::new()
    } else {
        format!("  [{lifecycle}]")
    };
    let availability = if teams_available {
        String::new()
    } else {
        "  [team details unavailable]".to_string()
    };
    out.push_str(&format!(
        "{}team{}  {}{}{}{}\n",
        "  ".repeat(depth),
        name,
        team_id,
        plan,
        state,
        availability
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
            truncate_title(&tab.label),
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

fn truncate_title(value: &str) -> String {
    const MAX_CHARS: usize = 32;
    if value.chars().count() <= MAX_CHARS {
        return value.to_string();
    }
    value.chars().take(MAX_CHARS - 1).chain(['…']).collect()
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_workspace_metadata_multiple_teams_and_teamless_tabs() {
        let workspaces = json!([
            {"id":"ws_code","name":"codeterm","tab_count":4,"cwd":"D:/Developer/codeterm","protected":true,"tab_ids":["orch","worker","other","shell"]},
            {"id":"ws_empty","name":"empty","tab_count":0,"cwd":"D:/empty","is_active":true,"tab_ids":[]}
        ]);
        let tabs = json!([
            {"id":"worker","tab_title":"Docs audit","workspace_id":"ws_code","team_id":"team_cli","parent_tab_id":"orch","agent_session_model":"sonnet","role_profile":"worker","status":"working"},
            {"id":"orch","tab_title":"CLI audit","workspace_id":"ws_code","team_id":"team_cli","agent_session_model":"opus","role_profile":"orchestrator","status":"idle"},
            {"id":"other","tab_title":"A second independent team with a title that is too long","workspace_id":"ws_code","team_id":"team_other"},
            {"id":"shell","tab_title":"logs","workspace_id":"ws_code"}
        ]);
        let teams = json!({"teams":[
            {"id":"team_cli","name":"CLI council","workspace_id":"ws_code","lifecycle":"active"},
            {"id":"team_other","workspace_id":"ws_code","lifecycle":"active"}
        ]});
        let plans = json!({"plans":[{"plan_id":"pl_cli","team_id":"team_cli","status":"active"}]});

        let output = render(
            &workspaces,
            Some(&tabs),
            Some(&teams),
            Some(&plans),
            Some("ws_code"),
            Some("orch"),
        );

        assert_eq!(
            output,
            "codeterm  ws_code  4 tabs  D:/Developer/codeterm  (self, protected)\n  team CLI council  team_cli  plan=pl_cli\n    orch  CLI audit  [opus · orchestrator · idle]  (self)\n      worker  Docs audit  [sonnet · worker · working]\n  team  team_other\n    other  A second independent team with …\n  (no team)\n    shell  logs\n\nempty  ws_empty  0 tabs  D:/empty  (active, empty)\n"
        );
        assert_eq!(output.matches("ws_code").count(), 1);
    }

    #[test]
    fn places_live_team_with_its_tabs_instead_of_stale_registered_workspace() {
        let workspaces =
            json!([{"id":"ws_live","name":"live","tab_count":1,"tab_ids":["orch"]}]);
        let tabs =
            json!([{"id":"orch","title":"audit","workspace_id":"ws_live","team_id":"team_cli"}]);
        let teams =
            json!({"teams":[{"id":"team_cli","workspace_id":"ws_deleted","lifecycle":"forming"}]});

        let plans = json!({"plans":[]});
        let output = render(
            &workspaces,
            Some(&tabs),
            Some(&teams),
            Some(&plans),
            None,
            None,
        );

        assert!(
            output.contains("live  ws_live  1 tab\n  team  team_cli  [forming]\n    orch  audit\n")
        );
        assert!(!output.contains("(missing workspace)"));
    }

    #[test]
    fn exposes_team_whose_workspace_and_tabs_are_gone() {
        let output = render(
            &json!([]),
            Some(&json!([])),
            Some(&json!({"teams":[{"id":"team_lost","workspace_id":"ws_gone","lifecycle":"forming"}]})),
            Some(&json!({"plans":[]})),
            None,
            None,
        );

        assert_eq!(
            output,
            "(missing workspace)\n  team  team_lost  [forming]\n"
        );
    }

    #[test]
    fn distinguishes_empty_hidden_and_unavailable_tab_data() {
        let workspaces = json!([
            {"id":"ws_empty","name":"empty","tab_count":0,"tab_ids":[]},
            {"id":"ws_hidden","name":"hidden","tab_count":2,"tab_ids":["hidden_one","hidden_two"]}
        ]);
        let teams = json!({"teams":[]});
        let plans = json!({"plans":[]});
        let available = render(
            &workspaces,
            Some(&json!([])),
            Some(&teams),
            Some(&plans),
            None,
            None,
        );

        assert!(available.contains("empty  ws_empty  0 tabs  (empty)"));
        assert!(available.contains("hidden  ws_hidden  2 tabs  (2 not shown)"));

        let output = render(
            &json!([{"id":"ws_one","name":"one","tab_count":2,"tab_ids":["a","b"]}]),
            None,
            Some(&teams),
            Some(&plans),
            None,
            None,
        );

        assert_eq!(output, "one  ws_one  2 tabs  (tabs unavailable)\n");
    }

    #[test]
    fn auxiliary_failures_keep_the_workspace_and_tab_spine() {
        let workspaces =
            json!([{"id":"ws","name":"one","tab_count":1,"tab_ids":["tab"]}]);
        let tabs = json!([{"id":"tab","title":"work","workspace_id":"ws","team_id":"team"}]);

        let output = render(&workspaces, Some(&tabs), None, None, None, None);

        assert!(output.contains("one  ws  1 tab"));
        assert!(output.contains("(teams unavailable)"));
        assert!(output.contains("team  team  plan=?  [team details unavailable]"));
        assert!(output.contains("tab  work"));
    }

    #[test]
    fn tab_siblings_follow_workspace_tab_id_creation_order() {
        let workspaces = json!([{
            "id":"ws","name":"one","tab_count":3,"tab_ids":["second","first","third"]
        }]);
        let tabs = json!([
            {"id":"first","title":"first","workspace_id":"ws"},
            {"id":"third","title":"third","workspace_id":"ws"},
            {"id":"second","title":"second","workspace_id":"ws"}
        ]);
        let teams = json!({"teams":[]});
        let plans = json!({"plans":[]});

        let output = render(
            &workspaces,
            Some(&tabs),
            Some(&teams),
            Some(&plans),
            None,
            None,
        );

        let second = output.find("second  second").unwrap();
        let first = output.find("first  first").unwrap();
        let third = output.find("third  third").unwrap();
        assert!(second < first && first < third);
    }

    #[test]
    fn fixed_input_renders_deterministically() {
        let workspaces =
            json!([{"id":"ws","name":"one","tab_count":1,"tab_ids":["tab"]}]);
        let tabs = json!([{"id":"tab","title":"work","workspace_id":"ws"}]);
        let teams = json!({"teams":[]});
        let plans = json!({"plans":[]});

        let first = render(
            &workspaces,
            Some(&tabs),
            Some(&teams),
            Some(&plans),
            None,
            None,
        );
        let second = render(
            &workspaces,
            Some(&tabs),
            Some(&teams),
            Some(&plans),
            None,
            None,
        );

        assert_eq!(first, second);
    }
}
