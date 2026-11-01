use crate::service::test_fixture::ServiceFixture;
use codeterm_plugin::js_runtime::{call_json, JsChatBackend, JsProvider, PluginManifest, PluginVm};
use serde_json::json;
use std::sync::Arc;

const ID: &str = "sync-send-fixture";

pub(super) fn register(fixture: &ServiceFixture, outcome: &str) -> Arc<PluginVm> {
    let manifest_json = json!({
        "id":ID,"version":"1.0.0","displayName":"Fixture",
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},"agentRuntime":fixture.service.agent_registry.provider_arc(ID).unwrap().agent_runtime(),"remoteTask":{
            "mode":"cloud","displayName":"Cloud","backend":"fixture",
            "availability":{"status":"unavailable","reason":"Sign in"},
            "environments":[],"followUp":false,"cancel":false,"applyDiff":false
        }},
        "permissions":{"subprocess":{"allow":["fixture-cli"]}}
    }).to_string();
    let manifest = PluginManifest::from_json_str(&manifest_json).unwrap();
    let source = format!(
        r#"
        let calls = 0;
        Object.assign(module.exports, {{
            discoverRemoteTask() {{
                calls++;
                if ('{outcome}' === 'error') throw new Error('opaque failure');
                return {{availability:{{status:'available'}},environments:'{outcome}' === 'empty' ? [] : [
                    {{id:'env-one',label:'One',accountRef:'opaque-account'}},
                    {{id:'env-two',label:'Two',accountRef:'opaque-account'}}
                ]}};
            }},
            discoveryCalls() {{ return calls; }}
        }});
    "#
    );
    let source = format!("{}\n{source}", super::SOURCE);
    let vm = Arc::new(PluginVm::from_source_with(ID, source, Some(manifest_json)).unwrap());
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(manifest.clone(), vm.clone())));
    fixture.service.chatbackend_registry.register(
        ID,
        Arc::new(JsChatBackend::from_manifest(vm.clone(), &manifest)),
    );
    vm
}

pub(super) fn assert_calls(vm: &PluginVm, expected: usize) {
    assert_eq!(
        call_json::<usize>(vm, "discoveryCalls", &[]).unwrap(),
        expected
    );
}

pub(super) fn assert_snapshot(fixture: &ServiceFixture, outcome: &str) {
    use crate::service::{
        spawn::{SpawnTabSpec, TabCommand},
        SpawnAgentParams,
    };
    let remote = fixture
        .service
        .chatbackend_registry
        .descriptor(ID)
        .unwrap()
        .remote_task
        .unwrap();
    let listed = fixture
        .service
        .provider_registry
        .describe_all()
        .into_iter()
        .find(|provider| provider.id == ID)
        .unwrap();
    assert_eq!(listed.remote_task.as_ref(), Some(&remote));
    assert_eq!(
        fixture
            .service
            .remote_execution(ID, &json!({"executionMode":"local"})),
        Ok(false)
    );
    let remote_spawn = fixture
        .service
        .remote_execution(ID, &json!({"executionMode":"cloud","preset":"env-one"}));
    if outcome == "many" {
        assert_eq!(remote_spawn, Ok(true));
        let params = SpawnAgentParams {
            agent: Some(ID.into()),
            execution_mode: Some("cloud".into()),
            preset: Some("env-one".into()),
            ..Default::default()
        };
        assert_eq!(fixture.service.validate_remote_spawn(&params), Ok(true));
        let spec = SpawnTabSpec::from_agent_params_with_chatbackends(
            &fixture.service.chatbackend_registry,
            params,
        );
        let TabCommand::ChatBackend {
            plugin_id, config, ..
        } = spec.command
        else {
            panic!("Cloud spawn must use the remote backend");
        };
        assert_eq!(plugin_id, ID);
        assert_eq!(config["executionMode"], "cloud");
        assert_eq!(config["preset"], "env-one");
    } else {
        assert!(remote_spawn.is_err());
    }
    assert!(fixture
        .service
        .remote_execution(ID, &json!({"executionMode":"cloud","preset":"foreign-env"}))
        .is_err());
    let snapshot = serde_json::to_value(remote).unwrap();
    assert_eq!(
        snapshot["availability"]["status"],
        if outcome == "error" {
            "unavailable"
        } else {
            "available"
        }
    );
    assert_eq!(
        snapshot["environments"],
        if outcome == "many" {
            json!([
                {"id":"env-one","label":"One","accountRef":"opaque-account"},
                {"id":"env-two","label":"Two","accountRef":"opaque-account"}
            ])
        } else {
            json!([])
        }
    );
}
