use crate::client::{RpcError, SocketCallError};
use crate::cmd::print::{exit_with_cli_error, CliError};

pub(crate) fn render_model_catalogue_error(data: &serde_json::Value) -> Option<String> {
    let error_type = data.get("type")?.as_str()?;
    let provider = data.get("provider")?.as_str()?;
    let message = data
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("invalid model selection");
    if error_type == "ambiguous_model" {
        let providers = data
            .get("ambiguous_providers")
            .and_then(serde_json::Value::as_array)
            .map(|providers| {
                providers
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|providers| !providers.is_empty())
            .unwrap_or_else(|| "provider id".into());
        return Some(format!(
            "✗ {message}\nfix: choose a provider explicitly (use --agent on dispatch or a positional provider on spawn): {providers}"
        ));
    }
    if error_type != "invalid_model" {
        return None;
    }
    let refresh = data
        .get("refresh_command")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("domios agent models {provider}"));
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
    match error {
        SocketCallError::Rpc(rpc) => format!(
            "Error: {}",
            crate::cmd::print::rpc_explanation(&rpc.message)
        ),
        SocketCallError::Transport(message) => format!("Error: {message}"),
    }
}

pub(crate) fn exit_with_socket_error(json: bool, error: SocketCallError) -> ! {
    if json {
        exit_with_cli_error(true, &CliError::from(error));
    }
    eprintln!("{}", render_socket_error(&error));
    std::process::exit(1)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "model_error/plan_limit_tests.rs"]
mod plan_limit_tests;
