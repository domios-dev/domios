use crate::client::{RpcError, SocketCallError};

pub(crate) fn render_model_catalogue_error(data: &serde_json::Value) -> Option<String> {
    if data.get("type")?.as_str()? != "invalid_model" {
        return None;
    }
    let provider = data.get("provider")?.as_str()?;
    let message = data
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("invalid model selection");
    Some(format!(
        "✗ {message}\nfix: codeterm agent models {provider}"
    ))
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

#[cfg(test)]
mod tests {
    use super::render_model_catalogue_error;
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
}
