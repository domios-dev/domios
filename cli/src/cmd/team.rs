use serde_json::{json, Value};

use crate::args::team::TeamAction;
use crate::client::RpcClient;

pub fn handle(client: &mut dyn RpcClient, json_output: bool, action: TeamAction) {
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
    if let Some(team) = value.get("team") {
        print_team(team);
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
}
