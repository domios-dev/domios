use crate::{
    js_runtime::{JsViewProvider, PluginManifest, PluginVm},
    media_delivery::{MediaDeliveryDescriptor, MediaFile, MediaType},
    view_host::{ViewError, ViewProvider},
};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
struct Native;
#[async_trait]
impl ViewProvider for Native {
    fn id(&self) -> &str {
        "fixture"
    }
    fn media_delivery(&self) -> Option<MediaDeliveryDescriptor> {
        Some(descriptor())
    }
    async fn media_file(
        &self,
        id: &str,
        context: &crate::view_host::ViewCallContext,
    ) -> Result<MediaFile, ViewError> {
        if id != "own" || context.plugin != "fixture" || context.owner.as_deref() != Some("owner") {
            return Err(ViewError::Call("denied".into()));
        }
        Ok(MediaFile {
            path: "generated/clip.mp4".into(),
            content_type: MediaType::Mp4,
            file_name: Some("Clip.mp4".into()),
        })
    }
    async fn call(
        &self,
        _: &str,
        _: Value,
        _context: &crate::view_host::ViewCallContext,
    ) -> Result<Value, ViewError> {
        Ok(Value::Null)
    }
}
fn descriptor() -> MediaDeliveryDescriptor {
    MediaDeliveryDescriptor {
        types: vec![MediaType::Mp4, MediaType::Png],
    }
}
#[tokio::test]
async fn native_and_js_media_ports_agree_and_undeclared_export_stays_denied() {
    let vm=Arc::new(PluginVm::from_source("fixture",r#"module.exports.default={mediaFile(id,context){if(id!=="own"||context.plugin!=="fixture"||context.owner!=="owner")throw Error("denied");return {path:"generated/clip.mp4",contentType:"video/mp4",fileName:"Clip.mp4"}}}"#.into()).unwrap());
    let mut context = crate::view_host::ViewCallContext::panel("fixture");
    context.owner = Some("owner".into());
    let undeclared = JsViewProvider::new(vm.clone(), "fixture");
    assert!(undeclared.media_delivery().is_none());
    assert!(undeclared.media_file("own", &context).await.is_err());
    let js = JsViewProvider::new(vm, "fixture").with_media_delivery(Some(descriptor()));
    assert_eq!(Native.media_delivery(), js.media_delivery());
    assert_eq!(
        Native.media_file("own", &context).await.unwrap(),
        js.media_file("own", &context).await.unwrap()
    );
    assert!(Native.media_file("foreign", &context).await.is_err());
    assert!(js.media_file("foreign", &context).await.is_err());
    let mut wrong = context;
    wrong.owner = Some("foreign".into());
    assert!(Native.media_file("own", &wrong).await.is_err());
    assert!(js.media_file("own", &wrong).await.is_err());
}
#[test]
fn manifest_media_descriptor_is_closed_and_requires_view() {
    let manifest = |caps: Value| {
        PluginManifest::from_json_str(&serde_json::json!({"id":"fixture","version":"1.0.0","displayName":"Fixture","capabilities":caps}).to_string())
    };
    let valid = manifest(
        serde_json::json!({"view":true,"mediaDelivery":{"types":["video/mp4","image/png"]}}),
    )
    .unwrap();
    assert_eq!(valid.capabilities.media_delivery, Some(descriptor()));
    for caps in [
        serde_json::json!({"mediaDelivery":{"types":["video/mp4"]}}),
        serde_json::json!({"view":true,"mediaDelivery":{"types":[]}}),
        serde_json::json!({"view":true,"mediaDelivery":{"types":["video/mp4","video/mp4"]}}),
        serde_json::json!({"view":true,"mediaDelivery":{"types":["text/html"]}}),
        serde_json::json!({"view":true,"mediaDelivery":{"types":["image/svg+xml"]}}),
    ] {
        assert!(manifest(caps).is_err());
    }
}

struct NativeRun;
#[async_trait]
impl ViewProvider for NativeRun {
    fn id(&self) -> &str {
        "fixture"
    }
    fn media_delivery(&self) -> Option<MediaDeliveryDescriptor> {
        Some(descriptor())
    }
    async fn run_media_file(
        &self,
        id: &str,
        context: &crate::media_delivery::RunMediaLookupContext,
    ) -> Result<MediaFile, ViewError> {
        if id != "own"
            || !context.valid_for("fixture")
            || context.workspace != "workspace"
            || context.owner != "owner"
            || context.run != "run"
            || context.producer_operation != "operation"
            || context.producer_port != "output"
            || context.receiver_node != "receiver"
        {
            return Err(ViewError::Call("durable media lookup denied".into()));
        }
        Ok(MediaFile {
            path: "generated/clip.mp4".into(),
            content_type: MediaType::Mp4,
            file_name: Some("Clip.mp4".into()),
        })
    }
    async fn call(
        &self,
        _method: &str,
        _args: Value,
        _context: &crate::view_host::ViewCallContext,
    ) -> Result<Value, ViewError> {
        Ok(Value::Null)
    }
}
#[tokio::test]
async fn native_and_js_durable_media_ports_agree_while_the_default_port_and_foreign_context_refuse()
{
    use crate::media_delivery::{RunMediaLookupContext, RunMediaLookupOrigin};
    let context = RunMediaLookupContext {
        origin: RunMediaLookupOrigin::DurableRun,
        plugin: "fixture".into(),
        workspace: "workspace".into(),
        team: None,
        owner: "owner".into(),
        run: "run".into(),
        producer_operation: "operation".into(),
        producer_port: "output".into(),
        receiver_node: "receiver".into(),
    };
    let vm=Arc::new(PluginVm::from_source("fixture",r#"module.exports.default={mediaFile(id,c){if(id!=="own"||c.origin!=="durable_run"||c.plugin!=="fixture"||c.workspace!=="workspace"||c.owner!=="owner"||c.run!=="run"||c.producerOperation!=="operation"||c.producerPort!=="output"||c.receiverNode!=="receiver"||c.frame!==undefined)throw Error("denied");return {path:"generated/clip.mp4",contentType:"video/mp4",fileName:"Clip.mp4"};}}"#.into()).unwrap());
    let undeclared = JsViewProvider::new(vm.clone(), "fixture");
    assert!(undeclared.run_media_file("own", &context).await.is_err());
    assert!(Native.run_media_file("own", &context).await.is_err());
    let js = JsViewProvider::new(vm, "fixture").with_media_delivery(Some(descriptor()));
    assert!(js.run_media_file("own", &context).await.is_err());
    let expected = NativeRun.run_media_file("own", &context).await.unwrap();
    let lookup_context = context.clone();
    let result = tokio::task::spawn_blocking(move || {
        let _scope = codeterm_decision::physical::RequestScope::observation_only(
            std::time::Instant::now() + std::time::Duration::from_secs(2),
        );
        futures_executor::block_on(js.run_media_file("own", &lookup_context))
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result, expected);
    let mut foreign = context;
    foreign.plugin = "foreign".into();
    assert!(NativeRun.run_media_file("own", &foreign).await.is_err());
}
