use super::*;
use codeterm_plugin::js_runtime::{call_json, JsChatBackend, JsProvider, PluginManifest, PluginVm};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use serde_json::{json, Value};
use std::sync::Arc;

#[tokio::test]
async fn explicit_refresh_discovers_remote_environments_for_dual_registered_provider() {
    for outcome in ["empty", "many", "error"] {
        let fixture = ServiceFixture::new(MultiplexerState::empty());
        let id = format!("dual-refresh-{outcome}");
        let manifest_json = json!({
            "id":id,"version":"1.0.0","displayName":"Fixture",
            "capabilities":{"agentProvider":true,"remoteTask":{
                "mode":"cloud","backend":"fixture","displayName":"Cloud",
                "availability":{"status":"unavailable","reason":"Sign in"},
                "environments":[],"followUp":false,"cancel":false,"applyDiff":false
            }}
        })
        .to_string();
        let manifest = PluginManifest::from_json_str(&manifest_json).unwrap();
        let source = format!(
            r#"
            let calls = 0;
            module.exports = {{
                discoverModels() {{ return [{{id:'opaque-local',displayName:'Local'}}]; }},
                discoverRemoteTask() {{
                    calls++;
                    if ('{outcome}' === 'error') throw new Error('opaque failure');
                    return {{availability:{{status:'available'}},environments:'{outcome}' === 'empty' ? [] : [
                        {{id:'env-a',label:'A',accountRef:'account'}},
                        {{id:'env-b',label:'B',accountRef:'account'}}
                    ]}};
                }},
                discoveryCalls() {{ return calls; }}
            }};
        "#
        );
        let vm = Arc::new(PluginVm::from_source_with(&id, source, Some(manifest_json)).unwrap());
        fixture
            .service
            .agent_registry
            .register_provider(Box::new(JsProvider::new(manifest.clone(), vm.clone())));
        fixture.service.chatbackend_registry.register(
            &id,
            Arc::new(JsChatBackend::from_manifest(vm.clone(), &manifest)),
        );
        let state = crate::test_support::app_state(&fixture);
        for expected_calls in [1, 2] {
            let _refreshed = refresh_agent_provider_models(
                AxumState(state.clone()),
                HeaderMap::new(),
                Json(RefreshProviderModelsBody {
                    provider_id: id.clone(),
                }),
            )
            .await
            .unwrap();
            assert_eq!(
                call_json::<usize>(&vm, "discoveryCalls", &[]).unwrap(),
                expected_calls
            );
            let remote = state
                .svc
                .chatbackend_registry
                .descriptor(&id)
                .unwrap()
                .remote_task
                .unwrap();
            let listed = state
                .svc
                .provider_registry
                .describe_all()
                .into_iter()
                .find(|provider| provider.id == id)
                .unwrap();
            assert_eq!(listed.remote_task.as_ref(), Some(&remote));
            assert_eq!(listed.available_models.len(), 1);
            assert_eq!(listed.available_models[0].id, "opaque-local");
            let remote = serde_json::to_value(remote).unwrap();
            assert_eq!(
                remote["environments"].as_array().unwrap().len(),
                if outcome == "many" { 2 } else { 0 }
            );
            assert_eq!(
                remote["availability"]["status"],
                if outcome == "error" {
                    "unavailable"
                } else {
                    "available"
                }
            );
            assert_eq!(
                remote["environments"].get(0).map(|row| row["id"].clone()),
                if outcome == "many" {
                    Some(Value::String("env-a".into()))
                } else {
                    None
                }
            );
        }
        fixture.service.chatbackend_registry.unregister(&id);
        codeterm_plugin::model_catalog::unregister(&id);
    }
}
