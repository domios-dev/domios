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
    async fn media_file(&self, id: &str) -> Result<MediaFile, ViewError> {
        if id != "own" {
            return Err(ViewError::Call("denied".into()));
        }
        Ok(MediaFile {
            path: "generated/clip.mp4".into(),
            content_type: MediaType::Mp4,
            file_name: Some("Clip.mp4".into()),
        })
    }
    async fn call(&self, _: &str, _: Value) -> Result<Value, ViewError> {
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
    let vm=Arc::new(PluginVm::from_source("fixture",r#"module.exports.default={mediaFile(id){if(id!=="own")throw Error("denied");return {path:"generated/clip.mp4",contentType:"video/mp4",fileName:"Clip.mp4"}}}"#.into()).unwrap());
    let undeclared = JsViewProvider::new(vm.clone(), "fixture");
    assert!(undeclared.media_delivery().is_none());
    assert!(undeclared.media_file("own").await.is_err());
    let js = JsViewProvider::new(vm, "fixture").with_media_delivery(Some(descriptor()));
    assert_eq!(Native.media_delivery(), js.media_delivery());
    assert_eq!(
        Native.media_file("own").await.unwrap(),
        js.media_file("own").await.unwrap()
    );
    assert!(Native.media_file("foreign").await.is_err());
    assert!(js.media_file("foreign").await.is_err());
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
