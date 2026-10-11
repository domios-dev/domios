use crate::args::rest_api::RestApiAction;

pub fn rpc_method(action: &RestApiAction) -> &'static str {
    match action {
        RestApiAction::Status => "rest_api.status",
        RestApiAction::Restart => "rest_api.restart",
    }
}

pub fn render_status(value: &serde_json::Value) -> String {
    let phase = value["phase"].as_str().unwrap_or("unknown");
    let generation = value["generation"].as_u64().unwrap_or_default();
    let mut output = format!("REST API: {phase} (generation {generation})\n");
    if let Some(diagnostic) = value["diagnostic"].as_str() {
        output.push_str(&format!("diagnostic: {diagnostic}\n"));
    }
    if let Some(message) = value["message"].as_str() {
        output.push_str(&format!("{message}\n"));
    }
    if phase == "failed" {
        if value["diagnostic"] == "bind_ownership_failed" {
            output.push_str(
                "fix: release the conflicting port, then run codeterm rest-api restart\n",
            );
        } else {
            output.push_str("fix: codeterm rest-api restart\n");
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn rest_api_commands_route_to_socket_methods() {
        for (verb, method) in [
            ("status", "rest_api.status"),
            ("restart", "rest_api.restart"),
        ] {
            let cli = crate::Cli::try_parse_from(["codeterm", "rest-api", verb]).unwrap();
            let crate::Commands::RestApi { action } = cli.command else {
                panic!("wrong command")
            };
            assert_eq!(rpc_method(&action), method);
            let transport = crate::transport::transport_for(&crate::Commands::RestApi { action });
            assert_eq!(transport.local, crate::transport::Local::Socket);
            assert!(matches!(
                transport.api_verdict,
                crate::transport::ApiVerdict::Unsupported(_)
            ));
        }
    }

    #[test]
    fn rest_api_status_retains_bind_error() {
        let value = serde_json::json!({ "generation": 3, "phase": "failed", "diagnostic": "bind_ownership_failed", "message": "fixture bind error" });
        let output = render_status(&value);
        assert!(output.contains(value["message"].as_str().unwrap()));
        assert!(output.contains(value["diagnostic"].as_str().unwrap()));
    }
}
