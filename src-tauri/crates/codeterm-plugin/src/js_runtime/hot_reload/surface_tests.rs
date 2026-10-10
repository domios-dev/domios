use std::sync::Arc;

use super::tests::test_caps;
use crate::js_runtime::{PluginManifest, PluginVm};

fn surface_manifest(view: bool) -> PluginManifest {
    serde_json::from_value(serde_json::json!({
        "id": "surface-fixture",
        "version": "0",
        "displayName": "Surface fixture",
        "capabilities": {
            "view": view,
            "surfaces": [{ "id": "board", "title": "Board", "renderer": "view" }]
        }
    }))
    .expect("manifest")
}

fn vm() -> Arc<PluginVm> {
    Arc::new(
        PluginVm::from_source(
            "surface-fixture",
            "module.exports.default = { viewCall() { return null; } };".into(),
        )
        .expect("vm"),
    )
}

#[test]
fn surface_registry_follows_plugin_registration() {
    let capabilities = test_caps();
    capabilities.register_plugin(&surface_manifest(true), &vm());
    let declared = capabilities
        .surfaces
        .get("surface-fixture")
        .expect("surfaces registered");
    assert_eq!(declared[0].id, "board");

    capabilities.deregister_plugin("surface-fixture");
    assert!(capabilities.surfaces.get("surface-fixture").is_none());
}

#[test]
fn surface_registry_skips_surfaces_without_a_view_to_render() {
    let capabilities = test_caps();
    capabilities.register_plugin(&surface_manifest(false), &vm());
    assert!(capabilities.surfaces.get("surface-fixture").is_none());
}

#[test]
fn surface_registry_registers_stream_surfaces_without_a_view() {
    let manifest: PluginManifest = serde_json::from_value(serde_json::json!({
        "id": "surface-fixture",
        "version": "0",
        "displayName": "Surface fixture",
        "capabilities": {
            "surfaces": [
                { "id": "board", "title": "Board", "renderer": "view" },
                { "id": "browser", "title": "Browser", "renderer": "stream", "runtime": "browser" }
            ]
        }
    }))
    .expect("manifest");
    let capabilities = test_caps();
    capabilities.register_plugin(&manifest, &vm());
    let declared = capabilities
        .surfaces
        .get("surface-fixture")
        .expect("stream surface registered");
    assert_eq!(declared.len(), 1);
    assert_eq!(declared[0].stream_runtime(), Some("browser"));
}
