use crate::args::agent_acp::{AcpAction, AcpRegistryAction};
use crate::client::SocketClient;
use serde_json::{json, Value};

pub(super) fn run(client: &mut SocketClient, json_output: bool, action: AcpAction) {
    let (operation, mut params) = match action {
        AcpAction::List => ("list", json!({})),
        AcpAction::Add {
            name,
            binary,
            args,
            env,
            secret_env,
            auth_env,
            like,
            client_capabilities,
            limits_source,
        } => (
            "add",
            json!({"name":name,"binary":binary,"args":args,"env":env,"secretEnv":secret_env,"authEnv":auth_env,
                "like":like,"clientCapabilities":client_capabilities,"limitsSource":limits_source}),
        ),
        AcpAction::Remove { name } => ("remove", json!({"name":name})),
        AcpAction::Registry {
            action: AcpRegistryAction::Search { query },
        } => ("registry_search", json!({"query":query})),
        AcpAction::Registry {
            action: AcpRegistryAction::Install { id, limits_source },
        } => (
            "registry_install",
            json!({"id":id,"limitsSource":limits_source}),
        ),
    };
    params["operation"] = operation.into();
    match client.call("agent.acp", Some(params)) {
        Ok(value) if json_output => crate::cmd::print::print_json(&value),
        Ok(Value::Array(rows)) => {
            for row in rows {
                println!(
                    "{}  {}",
                    row["id"].as_str().unwrap_or("?"),
                    if operation == "list" {
                        format!(
                            "install={} auth={}",
                            row["install"].as_str().unwrap_or("unknown"),
                            row["auth"].as_str().unwrap_or("unknown")
                        )
                    } else {
                        format!(
                            "{} — {}",
                            row["name"].as_str().unwrap_or(""),
                            row["description"].as_str().unwrap_or("")
                        )
                    }
                );
            }
        }
        Ok(value) => println!(
            "{}: {}",
            operation,
            value["id"].as_str().unwrap_or("complete")
        ),
        Err(error) => crate::cmd::print::fail(1, error.to_string()),
    }
}

#[cfg(test)]
#[path = "agent_acp/tests.rs"]
mod tests;
