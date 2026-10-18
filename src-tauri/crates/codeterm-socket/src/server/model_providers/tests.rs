use super::*;

fn cli_add_wire() -> Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/fixtures/model-provider/add.json"
    )))
    .unwrap()
}

#[test]
fn cli_add_wire_preserves_endpoint_and_opaque_key_through_preparation() {
    let wire = cli_add_wire();
    let (harness, request) = prepare("model_provider.add", Some(wire.clone())).unwrap();
    assert_eq!(harness, None);
    let ModelProviderRequest::Add { provider, value } = request else {
        panic!("wrong operation");
    };
    assert_eq!(serde_json::to_value(provider).unwrap(), wire["provider"]);
    assert!(Some(value.expose_secret()) == wire["value"].as_str());
}

#[test]
fn endpoint_operations_keep_all_fields_separate_from_harness_selection() {
    let mut endpoint = cli_add_wire()["provider"].clone();
    endpoint["keyEnv"] = json!("FIXTURE_KEY");
    endpoint["name"] = json!("Fixture endpoint");
    for action in ["add", "add_from_secret", "update"] {
        let mut wire = json!({"harness":"fixture-owner", "provider":endpoint});
        if action == "add_from_secret" {
            wire["source_secret"] = json!("source-slot");
        } else {
            wire["value"] = cli_add_wire()["value"].clone();
        }
        let (harness, request) =
            prepare(&format!("model_provider.{action}"), Some(wire.clone())).unwrap();
        assert_eq!(harness.as_deref(), Some("fixture-owner"));
        let provider = match request {
            ModelProviderRequest::Add { provider, value } => {
                assert!(Some(value.expose_secret()) == wire["value"].as_str());
                provider
            }
            ModelProviderRequest::AddFromSecret {
                provider,
                source_secret,
            } => {
                assert_eq!(source_secret, "source-slot");
                provider
            }
            ModelProviderRequest::Update { provider, value } => {
                assert!(Some(value.unwrap().expose_secret()) == wire["value"].as_str());
                provider
            }
            _ => panic!("wrong operation"),
        };
        assert_eq!(serde_json::to_value(provider).unwrap(), endpoint);
    }
    let (_, request) =
        prepare("model_provider.update", Some(json!({"provider":endpoint}))).unwrap();
    assert!(matches!(
        request,
        ModelProviderRequest::Update { value: None, .. }
    ));
}

#[test]
fn remaining_operations_preserve_the_typed_http_body_and_socket_scope() {
    for (method, body) in [
        (
            "discover",
            json!({"kind":"anthropic", "base_url":"https://example.com", "source_secret":"source-slot"}),
        ),
        ("list", json!({})),
        ("test", json!({"name":"demo"})),
        ("remove", json!({"name":"demo"})),
        (
            "preset.add",
            json!({"preset":{"name":"fixture", "model":"demo/fixture-model", "prompt":"opaque", "params":{"steps":12}}}),
        ),
        ("preset.remove", json!({"name":"fixture"})),
        ("preset.list", json!({})),
    ] {
        for scoped in [false, true] {
            let mut wire = body.clone();
            if scoped {
                wire["harness"] = json!("fixture-owner");
            }
            let (harness, request) =
                prepare(&format!("model_provider.{method}"), Some(wire)).unwrap();
            assert_eq!(harness.as_deref(), scoped.then_some("fixture-owner"));
            let mut http_body = body.clone();
            http_body["action"] = json!(method.replace('.', "_"));
            let http_request = serde_json::from_value::<ModelProviderRequest>(http_body).unwrap();
            assert_eq!(
                std::mem::discriminant(&request),
                std::mem::discriminant(&http_request)
            );
            match request {
                ModelProviderRequest::Discover {
                    kind,
                    base_url,
                    value,
                    source_secret,
                } => {
                    assert_eq!(serde_json::to_value(kind).unwrap(), body["kind"]);
                    assert_eq!(base_url, body["base_url"]);
                    assert!(value.is_none());
                    assert_eq!(source_secret.as_deref(), body["source_secret"].as_str());
                }
                ModelProviderRequest::List | ModelProviderRequest::PresetList => {
                    assert_eq!(body, json!({}))
                }
                ModelProviderRequest::Test { name }
                | ModelProviderRequest::Remove { name }
                | ModelProviderRequest::PresetRemove { name } => assert_eq!(name, body["name"]),
                ModelProviderRequest::PresetAdd { preset } => {
                    assert_eq!(serde_json::to_value(preset).unwrap(), body["preset"])
                }
                _ => panic!("wrong operation"),
            }
        }
    }
    assert!(matches!(
        prepare("model_provider.list", None).unwrap().1,
        ModelProviderRequest::List
    ));
}

#[test]
fn malformed_selectors_and_endpoint_objects_are_refused_without_echo() {
    for selector in [
        Value::Null,
        json!(false),
        json!(12),
        json!([]),
        json!({}),
        json!(""),
        json!(" owner"),
        json!("owner "),
    ] {
        for (method, mut wire) in [
            ("model_provider.add", cli_add_wire()),
            ("model_provider.list", json!({})),
        ] {
            wire["harness"] = selector.clone();
            let error = prepare(method, Some(wire)).err().unwrap();
            assert_eq!(error.code, -32602);
            assert_eq!(error.message, invalid().message);
            assert!(error.data.is_none());
        }
    }
    for action in ["add", "add_from_secret", "update"] {
        let mut wire = if action == "add_from_secret" {
            json!({"source_secret":"source-slot"})
        } else {
            cli_add_wire()
        };
        wire["provider"] = json!("fixture-owner");
        assert_eq!(
            prepare(&format!("model_provider.{action}"), Some(wire))
                .err()
                .unwrap()
                .code,
            -32602
        );
    }
}

#[test]
fn method_owns_action_and_malformed_params_never_reach_the_service() {
    for params in [
        json!([]),
        json!({"action":"remove", "name":"demo"}),
        json!({"unexpected":true}),
    ] {
        assert_eq!(
            prepare("model_provider.list", Some(params))
                .err()
                .unwrap()
                .code,
            -32602
        );
    }
    for params in [None, Some(Value::Null)] {
        assert!(matches!(
            prepare("model_provider.list", params).unwrap().1,
            ModelProviderRequest::List
        ));
    }
    assert_eq!(
        prepare("model_provider.unknown", None).err().unwrap().code,
        -32601
    );
}
