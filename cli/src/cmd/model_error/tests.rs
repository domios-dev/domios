use super::render_model_catalogue_error;
use crate::client::{RpcError, SocketCallError};
use crate::cmd::print::{write_cli_error, CliError};
use serde_json::json;

#[test]
fn structured_model_refusal_prints_valid_ids_and_refresh_command() {
    let error = json!({
        "type": "invalid_model",
        "provider": "codex",
        "requested_model": "gpt-9-bogus",
        "valid_model_ids": ["gpt-6-sol", "gpt-6-astra"],
        "message": "model id 'gpt-9-bogus' is not in provider 'codex' authoritative catalogue; valid model ids: gpt-6-sol, gpt-6-astra"
    });
    let rendered = render_model_catalogue_error(&error).unwrap();
    assert!(rendered.contains("gpt-9-bogus"));
    assert!(rendered.contains("gpt-6-sol"));
    assert!(rendered.contains("domios agent models codex"));
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
fn ambiguous_model_error_asks_for_a_provider_choice() {
    let error = json!({
        "type": "ambiguous_model",
        "provider": "opencode",
        "requested_model": "sonnet",
        "ambiguous_providers": ["claude", "opencode"],
        "message": "ambiguous model id 'sonnet': owned by claude, opencode; pass --agent to select a provider on dispatch, or provide it positionally to agent spawn"
    });
    let rendered = render_model_catalogue_error(&error).unwrap();
    assert!(rendered.contains("--agent on dispatch"), "{rendered}");
    assert!(rendered.contains("claude, opencode"), "{rendered}");
    assert!(!rendered.contains("domios agent models"), "{rendered}");
}

#[test]
fn spawn_json_model_refusal_keeps_catalogue_fields() {
    let data = json!({
        "type": "invalid_model",
        "provider": "codex",
        "requested_model": "gpt-9-bogus",
        "valid_model_ids": ["gpt-6-sol", "gpt-6-astra"],
        "refresh_command": "domios agent models codex",
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
    assert_eq!(output["result"]["type"], "invalid_model");
    assert_eq!(output["result"]["provider"], "codex");
    assert_eq!(output["result"]["requested_model"], "gpt-9-bogus");
    assert_eq!(output["result"]["valid_model_ids"][0], "gpt-6-sol");
    assert_eq!(
        output["result"]["refresh_command"],
        "domios agent models codex"
    );
    assert_eq!(output["error"]["rpc_code"], -32602);
    assert_eq!(output["error"]["code"], "invalid_argument");
}
