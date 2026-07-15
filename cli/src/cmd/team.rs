use serde_json::{json, Value};

use crate::args::team::TeamAction;
use crate::client::SocketClient;

pub fn handle(client: &mut SocketClient, json_output: bool, action: TeamAction) {
    let caller_pane_id = std::env::var("CODETERM_PANE_ID").ok();
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
                "caller_pane_id": caller_pane_id,
            })),
        ),
        TeamAction::AdoptPlan { team_id, plan_id } => client.call(
            "team.adopt_plan",
            Some(json!({
                "team_id": team_id,
                "plan_id": plan_id,
                "caller_pane_id": caller_pane_id,
            })),
        ),
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
