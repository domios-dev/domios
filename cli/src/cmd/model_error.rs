use crate::client::{RpcError, SocketCallError};
use crate::cmd::print::{exit_with_cli_error, CliError};

pub(crate) fn render_model_catalogue_error(data: &serde_json::Value) -> Option<String> {
    if data.get("type")?.as_str()? != "invalid_model" {
        return None;
    }
    let provider = data.get("provider")?.as_str()?;
    let message = data
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("invalid model selection");
    let refresh = data
        .get("refresh_command")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("codeterm agent models {provider}"));
    Some(format!("✗ {message}\nfix: {refresh}"))
}

pub(crate) fn render_socket_error(error: &SocketCallError) -> String {
    if let SocketCallError::Rpc(RpcError {
        data: Some(data), ..
    }) = error
    {
        if let Some(rendered) = render_model_catalogue_error(data) {
            return rendered;
        }
    }
    format!("Error: {error}")
}

pub(crate) fn exit_with_socket_error(json: bool, error: SocketCallError) -> ! {
    if json {
        exit_with_cli_error(true, &CliError::from(error));
    }
    eprintln!("{}", render_socket_error(&error));
    std::process::exit(1)
}

#[cfg(test)]
mod tests {
    use super::render_model_catalogue_error;
    use crate::client::{RpcError, SocketCallError};
    use crate::cmd::print::{write_cli_error, CliError};
    use serde_json::json;

    #[test]
    fn structured_model_refusal_prints_close_ids_and_refresh_command() {
        let error = json!({
            "type": "invalid_model",
            "provider": "codex",
            "requested_model": "gpt-9-bogus",
            "closest_model_ids": ["gpt-6-sol", "gpt-6-astra"],
            "message": "model id 'gpt-9-bogus' is not in provider 'codex' authoritative catalogue; closest catalogue ids: gpt-6-sol, gpt-6-astra"
        });
        let rendered = render_model_catalogue_error(&error).unwrap();
        assert!(rendered.contains("gpt-9-bogus"));
        assert!(rendered.contains("gpt-6-sol"));
        assert!(rendered.contains("codeterm agent models codex"));
    }

    #[test]
    fn prose_cannot_select_the_model_error_renderer() {
        assert!(render_model_catalogue_error(&json!({
            "type": "other_error",
            "message": "authoritative catalogue"
        }))
        .is_none());
    }

    #[test]
    fn spawn_json_model_refusal_keeps_catalogue_fields() {
        let data = json!({
            "type": "invalid_model",
            "provider": "codex",
            "requested_model": "gpt-9-bogus",
            "closest_model_ids": ["gpt-6-sol", "gpt-6-astra"],
            "refresh_command": "codeterm agent models codex",
            "message": "unknown model"
        });
        let error = CliError::from(SocketCallError::Rpc(RpcError {
            code: -32602,
            message: "unknown model".into(),
            data: Some(data),
        }));
        let mut out = Vec::new();
        let mut err = Vec::new();
        write_cli_error(true, &error, &mut out, &mut err);

        assert!(err.is_empty());
        let output: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(output["ok"], false);
        assert_eq!(output["kind"], "error");
        assert_eq!(output["data"]["type"], "invalid_model");
        assert_eq!(output["data"]["provider"], "codex");
        assert_eq!(output["data"]["requested_model"], "gpt-9-bogus");
        assert_eq!(output["data"]["closest_model_ids"][0], "gpt-6-sol");
        assert_eq!(
            output["data"]["refresh_command"],
            "codeterm agent models codex"
        );
        assert_eq!(output["code"], -32602);
    }
}
