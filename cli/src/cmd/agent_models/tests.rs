use super::*;

fn descriptors() -> Value {
    serde_json::json!([
        {"id": "other", "available_models": [{"id": "other-model"}]},
        {
            "id": "requested", "default_model": "model-b",
            "available_models": [
                {"id": "model-a", "display_name": "Model A", "availability": "unknown"},
                {"id": "model-b", "display_name": "Model B", "description": "Details",
                 "reasoning_efforts": [{"id": "high"}], "default_reasoning_effort": "high"}
            ]
        }
    ])
}

#[test]
fn shared_models_renderer_selects_only_requested_provider_rows() {
    let descriptors = descriptors();
    let output = render_agent_models(&descriptors, "requested", true).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&output).unwrap()["result"],
        descriptors[1]["available_models"]
    );
    assert_eq!(
        output,
        format!("{}\n", format_agent_models_json(&descriptors[1]))
    );
    assert_eq!(
        render_agent_models(&descriptors, "requested", false).unwrap(),
        format_agent_models(&descriptors[1], "requested")
    );
}

#[test]
fn shared_models_renderer_returns_typed_unknown_provider() {
    let descriptors = descriptors();
    for json in [false, true] {
        let error = render_agent_models(&descriptors, "missing", json).unwrap_err();
        assert_eq!(error, select_provider(&descriptors, "missing").unwrap_err());
        assert_eq!(error.data, Some(serde_json::json!({"code": "not_found"})));
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        crate::cmd::print::write_cli_error(json, &error, &mut stdout, &mut stderr);
        if json {
            let envelope: Value = serde_json::from_slice(&stdout).unwrap();
            assert_eq!(envelope["ok"], false);
            assert_eq!(envelope["error"]["code"], "not_found");
            assert!(stderr.is_empty());
        } else {
            assert!(stdout.is_empty());
            assert!(String::from_utf8(stderr).unwrap().contains(&error.message));
        }
        assert_eq!(
            error.message,
            "Unknown provider: 'missing'. Known: [other, requested]"
        );
    }
}

#[test]
fn shared_providers_renderer_preserves_descriptor_array_and_human_summary() {
    let descriptors = descriptors();
    assert_eq!(
        serde_json::from_str::<Value>(&render_agent_providers(&descriptors, true).unwrap())
            .unwrap()["result"],
        descriptors
    );
    let output = render_agent_providers(&descriptors, false).unwrap();
    assert!(output.contains("default=model-b  models=2"));
    assert!(output.contains("pty"));
    assert_eq!(
        render_agent_providers(&serde_json::json!([]), false).unwrap(),
        "No agent providers registered.\n"
    );
}
