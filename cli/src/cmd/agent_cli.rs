use crate::args::agent::ProviderCliAction;
use crate::client::SocketClient;

pub(super) fn run(client: &mut SocketClient, json: bool, action: ProviderCliAction) {
    let (provider, operation) = match action {
        ProviderCliAction::Status { provider } => (provider, None),
        ProviderCliAction::Install { provider } => (provider, Some("install")),
        ProviderCliAction::Update { provider } => (provider, Some("update")),
    };
    let result = match operation {
        None => client.call(
            "agents.cli_status",
            Some(serde_json::json!({"provider": provider})),
        ),
        Some(operation) => {
            let caller = crate::resolve_tab(client, None);
            client.call("agents.cli_run", Some(serde_json::json!({"provider": provider, "operation": operation, "caller_tab_id": caller})))
        }
    };
    match result {
        Ok(value) if json => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
        Ok(value) if operation.is_none() => {
            for line in crate::cmd::plugin::cli_status::status_lines(&provider, &value) {
                println!("{line}");
            }
            for key in ["installer", "updater"] {
                if let Some(command) = value.get(key).and_then(|v| v.as_str()) {
                    println!("{key}: {command}");
                }
            }
        }
        Ok(value) => {
            if let Some(warnings) = value.get("warnings").and_then(|value| value.as_array()) {
                for warning in warnings.iter().filter_map(|value| value.as_str()) {
                    println!("WARNING: {warning}");
                }
            }
            println!(
                "{} CLI {} dispatched in tab {}; completion will report back",
                provider,
                operation.unwrap_or_default(),
                value.get("tab_id").and_then(|v| v.as_str()).unwrap_or("?")
            );
        }
        Err(error) => crate::cmd::print::fail(1, error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    #[test]
    fn provider_cli_actions_parse_and_require_provider() {
        for verb in ["status", "install", "update"] {
            assert!(crate::args::cli::Cli::try_parse_from([
                "codeterm", "agent", "cli", verb, "fixture"
            ])
            .is_ok());
            assert!(
                crate::args::cli::Cli::try_parse_from(["codeterm", "agent", "cli", verb]).is_err()
            );
        }
        assert!(crate::args::cli::Cli::try_parse_from([
            "codeterm", "agent", "cli", "unknown", "fixture"
        ])
        .is_err());
    }
}
