use super::*;
struct Host;
impl PluginHostCtx for Host {
    fn runtime_launch_authorized(&self, binding: &RuntimeLaunchBinding) -> bool {
        binding.tab_id == "own-tab"
            && binding.provider_id == "runtime-fixture"
            && binding.binding_nonce == "own-nonce"
    }
    fn mem_search_json(&self, _opts_json: &str) -> String {
        unreachable!()
    }
    fn analytics_query_json(&self, _query_json: &str) -> String {
        unreachable!()
    }
    fn workspace_ensure_json(&self, _opts_json: &str) -> String {
        unreachable!()
    }
    fn workspace_tabs_json(&self, _workspace_id: &str) -> String {
        unreachable!()
    }
    fn close_tab(&self, _tab_id: &str) {
        unreachable!()
    }
    fn spawn_tab_json(&self, _spec_json: &str) -> String {
        unreachable!()
    }
    fn worker_get_json(&self, _session_id: &str) -> Option<String> {
        unreachable!()
    }
    fn worker_kill(&self, _session_id: &str) {
        unreachable!()
    }
    fn worker_pick_color(&self) -> String {
        unreachable!()
    }
    fn worker_register_json(&self, _info_json: &str) {
        unreachable!()
    }
    fn worker_start_json(&self, _plugin_id: &str, _opts_json: &str) -> String {
        unreachable!()
    }
    fn worker_poll_json(&self, _session_id: &str) -> String {
        unreachable!()
    }
    fn agent_spawn_json(&self, _plugin_id: &str, _workspace_id: &str, _opts_json: &str) -> String {
        unreachable!()
    }
    fn agent_send_json(&self, _plugin_id: &str, _session_id: &str, _text: &str) -> String {
        unreachable!()
    }
    fn agent_reap_json(&self, _session_id: &str) -> String {
        unreachable!()
    }
    fn agent_poll_json(&self, _session_id: &str) -> String {
        unreachable!()
    }
    fn progress_raw_json(&self, _plugin_id: &str, _opts_json: &str) -> String {
        unreachable!()
    }
    fn contributions_changed(&self, _kind: &str) {
        unreachable!()
    }
    fn emit_graph_worker_spawned_json(&self, _info_json: &str) {
        unreachable!()
    }
    fn socket_display_path(&self) -> String {
        unreachable!()
    }
    fn api_base_url(&self) -> String {
        unreachable!()
    }
    fn api_token(&self) -> Option<String> {
        unreachable!()
    }
    fn secret_get(&self, _plugin_id: &str, _name: &str) -> Option<String> {
        unreachable!()
    }
    fn secret_set(&self, _plugin_id: &str, _name: &str, _value: &str) -> bool {
        unreachable!()
    }
    fn secret_delete(&self, _plugin_id: &str, _name: &str) -> bool {
        unreachable!()
    }
}
#[test]
fn runtime_authority_requires_exact_host_verified_launch_binding() {
    let own = RuntimeLaunchBinding {
        tab_id: "own-tab".into(),
        provider_id: "runtime-fixture".into(),
        binding_nonce: "own-nonce".into(),
    };
    let proof = LaunchAuthority::verify(&Host, own.clone()).unwrap();
    assert!(proof.authorizes_provider("runtime-fixture"));
    assert!(!proof.authorizes_provider("foreign-provider"));
    for foreign in [
        RuntimeLaunchBinding {
            tab_id: "foreign-tab".into(),
            ..own.clone()
        },
        RuntimeLaunchBinding {
            binding_nonce: "foreign-nonce".into(),
            ..own.clone()
        },
        RuntimeLaunchBinding {
            provider_id: "foreign-provider".into(),
            ..own.clone()
        },
    ] {
        assert!(matches!(
            LaunchAuthority::verify(&Host, foreign),
            Err(RuntimeManifestError::ConsentRequired)
        ));
    }
}
