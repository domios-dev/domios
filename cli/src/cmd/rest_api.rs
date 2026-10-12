use crate::args::rest_api::RestApiAction;

pub fn rpc_method(action: &RestApiAction) -> &'static str {
    match action {
        RestApiAction::Status => "rest_api.status",
        RestApiAction::Restart => "rest_api.restart",
    }
}

pub fn command_result(
    action: &RestApiAction,
    result: Result<serde_json::Value, crate::client::SocketCallError>,
) -> Result<serde_json::Value, crate::cmd::print::CliError> {
    let value = result.map_err(crate::cmd::print::CliError::from)?;
    if value["ok"] == false
        || (matches!(action, RestApiAction::Restart) && value["phase"] == "failed")
    {
        let message = value["error"]["message"]
            .as_str()
            .or_else(|| value["error"].as_str())
            .or_else(|| value["message"].as_str())
            .unwrap_or("REST listener restart failed");
        return Err(crate::cmd::print::CliError {
            message: message.to_owned(),
            code: value["error"]["rpc_code"]
                .as_i64()
                .and_then(|code| code.try_into().ok()),
            data: Some(value),
        });
    }
    Ok(value)
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
    fn restart_failure_values_are_errors_but_failed_status_is_readable() {
        let failed = serde_json::json!({ "phase": "failed", "message": "fixture health failure" });
        assert_eq!(
            command_result(&RestApiAction::Status, Ok(failed.clone())).unwrap(),
            failed
        );
        let error = command_result(&RestApiAction::Restart, Ok(failed.clone())).unwrap_err();
        assert_eq!(error.message, failed["message"]);
        assert_eq!(error.data, Some(failed));
        let envelope = serde_json::json!({ "ok": false, "error": { "message": "fixture failure", "rpc_code": -32603 } });
        let error = command_result(&RestApiAction::Restart, Ok(envelope.clone())).unwrap_err();
        assert_eq!(error.message, envelope["error"]["message"]);
        assert_eq!(error.code, Some(-32603));
        assert_eq!(error.data, Some(envelope));
        let listening = serde_json::json!({ "phase": "listening", "generation": 2 });
        assert_eq!(
            command_result(&RestApiAction::Restart, Ok(listening.clone())).unwrap(),
            listening
        );
    }

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
