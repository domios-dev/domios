use super::*;
use crate::remote_tasks::RemoteTaskCapability;

struct RemoteNativeStub(RemoteTaskCapability);

impl ChatBackendProvider for RemoteNativeStub {
    fn id(&self) -> &str {
        "remote-fixture"
    }

    fn remote_task_capability(&self) -> Option<RemoteTaskCapability> {
        Some(self.0.clone())
    }

    fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
        unreachable!()
    }

    fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
        unreachable!()
    }

    fn poll(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<codeterm_types::agent::chat_types::StructuredChat, ChatBackendError> {
        unreachable!()
    }

    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        unreachable!()
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
        Ok(Vec::new())
    }
}

#[test]
fn remote_task_capability_native_js_parity_and_manifest_round_trip() {
    let capability = serde_json::json!({
        "backend":"fixture", "localProviderId":"local-fixture",
        "availability":{"status":"unavailable","reason":"Connect an account"},
        "environments":[],"followUp":false,"cancel":false,"applyDiff":false
    });
    let manifest_json = serde_json::json!({
        "id":"remote-fixture","version":"0.1.0","displayName":"Fixture",
        "capabilities":{"chatBackend":{"spawnable":true,"remoteTask":capability}}
    })
    .to_string();
    let manifest: super::super::PluginManifest = serde_json::from_str(&manifest_json).unwrap();
    let round_trip = serde_json::to_value(&manifest.capabilities.chat_backend).unwrap();
    assert_eq!(round_trip["remoteTask"], capability);
    let vm = super::super::PluginVm::from_source_with(
        "remote-fixture",
        "module.exports.default = {};".into(),
        Some(manifest_json),
    )
    .unwrap();
    let js = super::super::JsChatBackend::from_manifest(std::sync::Arc::new(vm), &manifest);
    let native = RemoteNativeStub(serde_json::from_value(capability).unwrap());
    assert_eq!(js.describe().remote_task, native.describe().remote_task);
    assert_eq!(
        js.describe().transport,
        crate::provider::ProviderTransport::ChatBackend
    );
}
