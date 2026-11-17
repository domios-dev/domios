use super::*;

#[test]
fn opencode_v2_shared_launch_fixture_uses_native_quickjs_version_selection() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../plugins/provider-opencode/tests/fixtures/cli-launch-contract.json"
    )))
    .unwrap();
    let provider =
        super::super::super::provider_observation::with_fixture_version(Some("2.3.0"), || {
            load_js("opencode")
        });
    let args = serde_json::from_value(fixture["modern"]["args"].clone()).unwrap();
    let command = provider.build_launch_command(&AgentLaunchParams {
        args: Some(args),
        skip_permissions: true,
        structured_prompt_server: Some(crate::provider::StructuredPromptServer {
            hostname: "127.0.0.1".into(),
            port: 53626,
        }),
        ..Default::default()
    });
    assert!(
        command.contains("opencode --standalone --auto"),
        "{command}"
    );
    assert!(command.contains(fixture["modern"]["config"]["model"].as_str().unwrap()));
    for unsupported in ["--hostname", "--port", "'-m'", "'--model'", "'--agent'"] {
        assert!(!command.contains(unsupported), "{command}");
    }
}
