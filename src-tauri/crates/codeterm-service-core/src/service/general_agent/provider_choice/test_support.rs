//! The ACP plugin's runtime capability, shared by the provider-choice tests here and in the
//! lifecycle crate.

pub fn profile_runtime() -> codeterm_plugin::agent_runtime::AgentRuntimeCapability {
    let manifest: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../plugins/provider-acp/plugin.json"
    )))
    .unwrap();
    serde_json::from_value(manifest["capabilities"]["agentRuntime"].clone()).unwrap()
}
