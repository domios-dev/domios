use serde_json::{json, Value};

use crate::args::team::TeamAction;
use crate::client::RpcClient;

pub fn handle(client: &mut dyn RpcClient, json_output: bool, action: TeamAction) {
    if let TeamAction::Pair {
        orchestrator,
        planner,
        workspace,
        title,
    } = action
    {
        handle_pair(
            client,
            json_output,
            &orchestrator,
            &planner,
            workspace,
            title,
        );
        return;
    }
    let caller_tab_id = (!client.is_remote())
        .then(|| std::env::var("CODETERM_TAB_ID").ok())
        .flatten();
    let result = match action {
        TeamAction::List => client.call("team.list", None),
        TeamAction::Show { team_id } => {
            client.call("team.show", Some(json!({ "team_id": team_id })))
        }
        TeamAction::Rename { team_id, name } => client.call(
            "team.rename",
            Some(json!({
                "team_id": team_id,
                "name": name,
                "caller_tab_id": caller_tab_id,
            })),
        ),
        TeamAction::Close { team_id } => client.call(
            "team.close",
            Some(json!({
                "team_id": team_id,
                "caller_tab_id": caller_tab_id,
            })),
        ),
        TeamAction::AdoptPlan { team_id, plan_id } => {
            adopt_plan_params(team_id, plan_id, caller_tab_id.as_deref())
                .and_then(|params| client.call("team.adopt_plan", Some(params)))
        }
        TeamAction::AdoptAgent { team_id, tab_id } => {
            adopt_agent_params(team_id, tab_id, caller_tab_id.as_deref())
                .and_then(|params| client.call("team.adopt_agent", Some(params)))
        }
        TeamAction::Pair { .. } => unreachable!("pair returns before the socket verbs"),
    };
    match result {
        Ok(value) if json_output => println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| "null".into())
        ),
        Ok(value) => print_human(&value),
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::exit(1);
        }
    }
}

fn handle_pair(
    client: &mut dyn RpcClient,
    json_output: bool,
    orchestrator: &str,
    planner: &str,
    workspace: Option<String>,
    title: Option<String>,
) {
    let params = pair_params(orchestrator, planner, workspace, title);
    match client.call("orchestration.new", Some(params)) {
        Ok(value) if json_output => println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| "null".into())
        ),
        Ok(value) => print_pair(&value),
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::exit(1);
        }
    }
}

/// Splits a `provider[:model]` spec. A trailing empty model (`"claude:"`) is
/// treated the same as no model — let the provider descriptor default apply.
pub(crate) fn split_provider_model(spec: &str) -> (String, Option<String>) {
    match spec.split_once(':') {
        Some((provider, model)) if !model.trim().is_empty() => {
            (provider.trim().to_string(), Some(model.trim().to_string()))
        }
        _ => (
            spec.split(':').next().unwrap_or(spec).trim().to_string(),
            None,
        ),
    }
}

pub(crate) fn pair_params(
    orchestrator: &str,
    planner: &str,
    workspace: Option<String>,
    title: Option<String>,
) -> Value {
    let (orchestrator_agent, orchestrator_model) = split_provider_model(orchestrator);
    let (planner_agent, planner_model) = split_provider_model(planner);

    let mut params = json!({
        "orchestrator_agent": orchestrator_agent,
        "planner_agent": planner_agent,
    });
    if let Some(m) = orchestrator_model {
        params["orchestrator_model"] = json!(m);
    }
    if let Some(m) = planner_model {
        params["planner_model"] = json!(m);
    }
    if let Some(w) = workspace {
        params["workspace_id"] = json!(w);
    }
    if let Some(t) = title {
        params["title"] = json!(t);
    }
    if let Ok(spawner) = std::env::var("CODETERM_TAB_ID") {
        if !spawner.is_empty() {
            params["spawned_by_tab_id"] = json!(spawner);
        }
    }
    params
}

fn print_pair(value: &Value) {
    let plan_id = value.get("plan_id").and_then(Value::as_str).unwrap_or("?");
    let orch_tab = value
        .get("orchestrator_tab_id")
        .and_then(Value::as_str)
        .unwrap_or("?");
    let planner_tab = value
        .get("planner_tab_id")
        .and_then(Value::as_str)
        .unwrap_or("?");
    println!("Plan: {plan_id}");
    println!("Orchestrator: Tab {orch_tab}");
    println!("Planner: Tab {planner_tab}");
    if let Some(warning) = value.get("warning").and_then(Value::as_str) {
        eprintln!("Warning: {warning}");
    }
}

fn adopt_plan_params(
    team_id: String,
    plan_id: String,
    caller_tab_id: Option<&str>,
) -> Result<Value, String> {
    let caller_tab_id = caller_tab_id
        .filter(|tab_id| !tab_id.trim().is_empty())
        .ok_or_else(|| {
            "Team plan adoption requires CODETERM_TAB_ID from the calling Team manager".to_string()
        })?;
    Ok(json!({
        "team_id": team_id,
        "plan_id": plan_id,
        "caller_tab_id": caller_tab_id,
    }))
}

fn adopt_agent_params(
    team_id: String,
    tab_id: String,
    caller_tab_id: Option<&str>,
) -> Result<Value, String> {
    let caller_tab_id = caller_tab_id
        .filter(|caller| !caller.trim().is_empty())
        .ok_or_else(|| {
            "Team agent adoption requires CODETERM_TAB_ID from a live Team member".to_string()
        })?;
    Ok(json!({
        "team_id": team_id,
        "tab_id": tab_id,
        "caller_tab_id": caller_tab_id,
    }))
}

fn print_human(value: &Value) {
    if let Some(teams) = value.get("teams").and_then(Value::as_array) {
        if teams.is_empty() {
            println!("No Teams.");
            return;
        }
        for team in teams {
            print_team(team);
        }
        return;
    }
    if let (Some(team_id), Some(tab_id)) = (
        value.get("team_id").and_then(Value::as_str),
        value.get("tab_id").and_then(Value::as_str),
    ) {
        println!("Adopted Tab {tab_id} into Team {team_id}.");
        return;
    }
    if let Some(team) = value.get("team") {
        print_team(team);
        print_members(
            value
                .get("members")
                .and_then(Value::as_array)
                .map(Vec::as_slice),
        );
        return;
    }
    let team_id = value.get("team_id").and_then(Value::as_str).unwrap_or("?");
    let plan_id = value
        .get("plan")
        .and_then(|plan| plan.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("?");
    println!("Adopted plan {plan_id} into Team {team_id}.");
}

fn print_members(members: Option<&[Value]>) {
    let Some(members) = members else { return };
    if members.is_empty() {
        println!("  (no member tabs)");
        return;
    }
    for member in members {
        let tab_id = member.get("tab_id").and_then(Value::as_str).unwrap_or("?");
        let title = member.get("title").and_then(Value::as_str).unwrap_or("");
        let role = member.get("role").and_then(Value::as_str).unwrap_or("-");
        println!("  {tab_id}\t{role}\t{title}");
    }
}

fn print_team(team: &Value) {
    let id = team.get("id").and_then(Value::as_str).unwrap_or("?");
    let name = team
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("unnamed");
    let lifecycle = team
        .get("lifecycle")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    println!("{id}\t{name}\t{lifecycle}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adopt_plan_params_require_nonempty_caller() {
        let params = adopt_plan_params("team".into(), "plan".into(), Some("manager")).unwrap();
        assert_eq!(params["team_id"], "team");
        assert_eq!(params["plan_id"], "plan");
        assert_eq!(params["caller_tab_id"], "manager");

        for caller in [None, Some(""), Some("  ")] {
            let error = adopt_plan_params("team".into(), "plan".into(), caller).unwrap_err();
            assert!(error.contains("CODETERM_TAB_ID"));
        }
    }

    #[test]
    fn adopt_agent_params_require_nonempty_caller() {
        let params =
            adopt_agent_params("team".into(), "tab_seven".into(), Some("manager")).unwrap();
        assert_eq!(params["team_id"], "team");
        assert_eq!(params["tab_id"], "tab_seven");
        assert_eq!(params["caller_tab_id"], "manager");

        for caller in [None, Some(""), Some("  ")] {
            let error = adopt_agent_params("team".into(), "tab_seven".into(), caller).unwrap_err();
            assert!(error.contains("CODETERM_TAB_ID"));
        }
    }

    #[test]
    fn splits_provider_and_model() {
        assert_eq!(
            split_provider_model("claude:fable"),
            ("claude".to_string(), Some("fable".to_string()))
        );
    }

    #[test]
    fn provider_only_has_no_model() {
        assert_eq!(split_provider_model("codex"), ("codex".to_string(), None));
    }

    #[test]
    fn trailing_colon_with_no_model_falls_back_to_provider_only() {
        assert_eq!(
            split_provider_model("claude:"),
            ("claude".to_string(), None)
        );
    }
}
