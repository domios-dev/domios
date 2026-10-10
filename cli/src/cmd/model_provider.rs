use crate::{
    args::model_provider::{EndpointKind, ModelPresetAction, ModelProviderAction},
    client::RpcClient,
};
use serde_json::{json, Value};
use std::io::{IsTerminal, Read};

fn read_key(reader: impl Read) -> Result<String, &'static str> {
    let mut value = String::new();
    reader
        .take(65537)
        .read_to_string(&mut value)
        .map_err(|_| "could not read key from stdin")?;
    let value = value.trim_end_matches(['\r', '\n']).to_owned();
    if value.is_empty() || value.len() > 65536 {
        return Err("stdin must contain a nonempty key of at most 65536 bytes");
    }
    Ok(value)
}

pub fn handle(client: &mut dyn RpcClient, json_output: bool, action: ModelProviderAction) {
    if client.is_remote() {
        crate::cmd::print::fail(1, "model provider secrets and configuration are local-only");
    }
    if matches!(&action, ModelProviderAction::Add { .. }) && std::io::stdin().is_terminal() {
        crate::cmd::print::fail(1, "pipe the key to model-provider add on stdin");
    }
    match run(client, action, std::io::stdin()) {
        Ok(value) if json_output => println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_default()
        ),
        Ok(value) => crate::cmd::print::print_object(&value),
        Err(error) => crate::cmd::print::fail_with(1, &error),
    }
}

fn run(
    client: &mut dyn RpcClient,
    action: ModelProviderAction,
    reader: impl Read,
) -> Result<Value, crate::cmd::print::CliError> {
    if client.is_remote() {
        return Err(crate::cmd::print::CliError::new(
            "model provider operations are local-only",
        ));
    }
    let (method, params) = match action {
        ModelProviderAction::Preset { action } => match action {
            ModelPresetAction::List => ("model_provider.preset.list", json!({})),
            ModelPresetAction::Remove { name } => {
                ("model_provider.preset.remove", json!({"name":name}))
            }
            ModelPresetAction::Add {
                name,
                model,
                prompt,
                temperature,
                top_p,
                steps,
            } => {
                let mut params = json!({});
                if let Some(value) = temperature {
                    params["temperature"] = json!(value);
                }
                if let Some(value) = top_p {
                    params["top_p"] = json!(value);
                }
                if let Some(value) = steps {
                    params["steps"] = json!(value);
                }
                (
                    "model_provider.preset.add",
                    json!({"preset":{"name":name,"model":model,"prompt":prompt,"params":params}}),
                )
            }
        },
        ModelProviderAction::Add {
            name,
            kind,
            base_url,
            secret,
            models,
        } => {
            let key = read_key(reader).map_err(crate::cmd::print::CliError::new)?;
            add_request(name, kind, base_url, secret, models, key)
        }
        ModelProviderAction::Remove { name } => ("model_provider.remove", json!({ "name": name })),
        ModelProviderAction::List => ("model_provider.list", json!({})),
        ModelProviderAction::Test { name } => ("model_provider.test", json!({ "name": name })),
    };
    client
        .call_detailed(method, Some(params))
        .map_err(crate::cmd::print::CliError::from)
}

fn add_request(
    name: String,
    kind: EndpointKind,
    base_url: String,
    secret: String,
    models: Vec<String>,
    key: String,
) -> (&'static str, Value) {
    let kind = match kind {
        EndpointKind::OpenaiCompatible => "openai-compatible",
        EndpointKind::Anthropic => "anthropic",
    };
    (
        "model_provider.add",
        json!({
            "provider": { "id": name, "kind": kind, "baseUrl": base_url, "secretName": secret, "models": models },
            "value": key,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Client {
        calls: Vec<(String, Value)>,
    }
    impl RpcClient for Client {
        fn call_detailed(
            &mut self,
            method: &str,
            params: Option<Value>,
        ) -> Result<Value, crate::client::SocketCallError> {
            self.calls.push((method.into(), params.unwrap()));
            Ok(json!({"models":["fixture-model"]}))
        }
    }

    #[test]
    fn model_provider_stdin_rpc_and_json_result_do_not_echo_the_key() {
        let mut client = Client { calls: vec![] };
        let key = ["opaque", "rpc", "fixture"].join("-");
        let result = run(
            &mut client,
            ModelProviderAction::Add {
                name: "demo".into(),
                kind: EndpointKind::OpenaiCompatible,
                base_url: "http://localhost:1234/v1".into(),
                secret: "slot".into(),
                models: vec!["fixture-model".into()],
            },
            format!("{key}\r\n").as_bytes(),
        )
        .unwrap();
        assert_eq!(client.calls.len(), 1);
        assert_eq!(client.calls[0].0, "model_provider.add");
        assert_eq!(client.calls[0].1["value"], key);
        assert_eq!(
            serde_json::to_string(&result).unwrap(),
            r#"{"models":["fixture-model"]}"#
        );
        assert!(!result.to_string().contains(&key));
    }

    #[test]
    fn model_provider_non_add_commands_do_not_read_stdin() {
        struct Unread;
        impl Read for Unread {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                panic!("stdin read");
            }
        }
        let mut client = Client { calls: vec![] };
        for action in [
            ModelProviderAction::List,
            ModelProviderAction::Remove {
                name: "demo".into(),
            },
            ModelProviderAction::Test {
                name: "demo".into(),
            },
            ModelProviderAction::Preset {
                action: ModelPresetAction::List,
            },
            ModelProviderAction::Preset {
                action: ModelPresetAction::Remove {
                    name: "demo".into(),
                },
            },
            ModelProviderAction::Preset {
                action: ModelPresetAction::Add {
                    name: "demo".into(),
                    model: "endpoint/model".into(),
                    prompt: "opaque".into(),
                    temperature: Some(0.2),
                    top_p: None,
                    steps: Some(12),
                },
            },
        ] {
            run(&mut client, action, Unread).unwrap();
        }
        assert_eq!(
            client.calls,
            vec![
                ("model_provider.list".into(), json!({})),
                ("model_provider.remove".into(), json!({"name":"demo"})),
                ("model_provider.test".into(), json!({"name":"demo"})),
                ("model_provider.preset.list".into(), json!({})),
                (
                    "model_provider.preset.remove".into(),
                    json!({"name":"demo"})
                ),
                (
                    "model_provider.preset.add".into(),
                    json!({"preset":{"name":"demo","model":"endpoint/model","prompt":"opaque","params":{"temperature":0.2,"steps":12}}})
                ),
            ]
        );
    }
    #[test]
    fn model_provider_stdin_is_bounded_and_removes_only_line_endings() {
        assert_eq!(read_key(" opaque\r\n".as_bytes()).unwrap(), " opaque");
        assert!(read_key("\r\n".as_bytes()).is_err());
        assert!(read_key("x".repeat(65537).as_bytes()).is_err());
    }
    #[test]
    fn model_provider_add_wire_keeps_key_out_of_provider_config() {
        let key = ["opaque", "fixture"].join("-");
        let (method, params) = add_request(
            "demo".into(),
            EndpointKind::Anthropic,
            "https://example.com".into(),
            "slot".into(),
            vec![],
            key.clone(),
        );
        assert_eq!(method, "model_provider.add");
        assert_eq!(params["value"], key);
        assert!(!params["provider"].to_string().contains(&key));
    }
}
