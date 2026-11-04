use super::*;
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::{test_fixture::ServiceFixture, SpawnAgentParams};
use std::sync::Arc;

#[test]
fn launch_profile_refusals_are_422_and_create_no_tab() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let provider = "profile-refusal-fixture";
    let manifest = PluginManifest::from_json_str(&serde_json::json!({
        "id": provider, "version": "1.0.0", "displayName": "Fixture",
        "capabilities": {"agentProvider": true,
            "chatBackend": {"spawnable": true, "sessionConfig": true, "agentTasks": true},
            "agentRuntime": {"runtime": "stdio-jsonrpc", "command": {"binary": "fixture", "args": []},
                "launchProfiles": {"selected": {"binary": "fixture", "args": [], "ownerConfigured": true}},
                "requestMethods": {"initialize": ["initialize"]}, "notificationMethods": []}},
        "permissions": {"subprocess": {"allow": ["fixture"]}}
    }).to_string()).unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            Arc::new(PluginVm::from_source(provider, "module.exports = {};".into()).unwrap()),
        )));
    let before = fixture.service.mux.read().snapshot().tabs.len();
    for (profile, code) in [
        (Some("unknown"), "unknown_launch_profile"),
        (None, "launch_profile_required"),
        (Some("selected"), "launch_profile_not_installed"),
    ] {
        let params = SpawnAgentParams {
            agent: Some(provider.into()),
            launch_profile: profile.map(str::to_owned),
            ..Default::default()
        };
        let error = fixture
            .service
            .validate_launch_profile(provider, params.launch_profile.as_deref())
            .unwrap_err();
        let refusal = SpawnRejection::from(error);
        assert_eq!(refusal.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(refusal.body["error"]["code"], code);
        assert_eq!(
            refusal.body["error"]["declared_profile_ids"],
            serde_json::json!(["selected"])
        );
        assert_eq!(
            refusal.body["error"]["requested_profile"],
            serde_json::json!(profile)
        );
        assert_eq!(fixture.service.mux.read().snapshot().tabs.len(), before);
    }
}
