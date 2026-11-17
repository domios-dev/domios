use crate::service::ServiceContext;

pub fn register_neighbor_runtime(svc: &ServiceContext) {
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    let manifest = PluginManifest::from_json_str(&serde_json::json!({
        "id":"pad-runtime","version":"1.0.0","displayName":"Fixture",
        "commands":{"autoApproveFlags":["opaque-full"]},
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
            "agentRuntime":{"runtimeModes":[{"id":"full-access","displayName":"Full"}],
                "runtime":"stdio-jsonrpc","command":{"binary":"fixture-cli","args":[]},
                "launchProfiles":{"approve-profile":{"binary":"fixture-cli","args":[],"fullAccessMode":"opaque-approve-all"}},
                "requestMethods":{"initialize":["handshake"]},"notificationMethods":[]}},
        "spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},"taskDelivery":"pty_input","verifySubmit":{"kind":"none"},
            "executionSandbox":{"platforms":["windows","linux","macos"],
                "readOnly":{"launchArgs":[],"runtimeMode":"opaque-read"},
                "runtimeReadOnlyWritableRootsMode":"opaque-pad",
                "mutating":{"launchArgs":[],"runtimeMode":"opaque-folder"}}},
        "permissions":{"subprocess":{"allow":["fixture-cli"]}}
    }).to_string()).unwrap();
    let vm = std::sync::Arc::new(
        PluginVm::from_source("pad-runtime", "module.exports={}".into()).unwrap(),
    );
    svc.agent_registry
        .register_provider(Box::new(JsProvider::new(manifest, vm)));
}
