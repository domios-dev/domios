use serde_json::json;

use crate::args::team::channel::TeamChannelAction;
use crate::client::RpcClient;

pub fn handle(client: &mut dyn RpcClient, json_output: bool, action: TeamChannelAction) {
    let params = match action {
        TeamChannelAction::Add { url, name } => {
            json!({ "action": "add", "url": url, "name": name })
        }
        TeamChannelAction::List => json!({ "action": "list" }),
        TeamChannelAction::Refresh { name } => json!({ "action": "refresh", "name": name }),
        TeamChannelAction::Remove { name } => json!({ "action": "remove", "name": name }),
    };
    match client.call_detailed("team.channel", Some(params)) {
        Ok(value) => {
            if json_output {
                crate::cmd::print::print_json(&value);
            } else if let Some(rows) = value["channels"].as_array() {
                for row in rows {
                    println!(
                        "{}  {} — {}",
                        row["name"].as_str().unwrap_or_default(),
                        row["url"].as_str().unwrap_or_default(),
                        row["lastRefresh"].as_str().unwrap_or("never refreshed")
                    );
                }
            }
        }
        Err(error) => crate::cmd::model_error::exit_with_socket_error(json_output, error),
    }
}
