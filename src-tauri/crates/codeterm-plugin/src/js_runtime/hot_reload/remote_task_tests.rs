use super::*;
use serde_json::json;

#[tokio::test]
async fn remote_tasks_keep_the_call_bridge_without_advertising_an_html_view() {
    let manifest: PluginManifest = serde_json::from_value(json!({
        "id":"remote-call-fixture","version":"1.0.0","displayName":"Fixture",
        "capabilities":{"chatBackend":{"spawnable":true},"remoteTask":{
            "backend":"fixture","mode":"cloud","displayName":"Cloud",
            "availability":{"status":"available"},"environments":[],
            "followUp":false,"cancel":false,"applyDiff":false
        }}
    }))
    .unwrap();
    assert!(!manifest.capabilities.view);
    let vm = Arc::new(
        super::super::PluginVm::from_source(
            "remote-call-fixture",
            "module.exports={viewCall(method,args){return {method,session:args.sessionId};}};"
                .into(),
        )
        .unwrap(),
    );
    let registries = tests::test_caps();
    registries.register_plugin(&manifest, &vm);
    let provider = registries.view.get("remote-call-fixture").unwrap();
    for method in ["read", "submit", "refresh"] {
        assert_eq!(
            provider
                .call(
                    method,
                    json!({"sessionId":"own-session"}),
                    &crate::view_host::ViewCallContext::panel("remote-call-fixture")
                )
                .await
                .unwrap(),
            json!({"method":method,"session":"own-session"})
        );
    }
    registries.deregister_plugin("remote-call-fixture");
    assert!(registries.view.get("remote-call-fixture").is_none());
}
