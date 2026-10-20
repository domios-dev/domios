//! Converter registration through the plugin lifecycle: `graphNodes.converters`
//! registers `JsConverter`, `transcriber` registers the audio -> text adapter,
//! and reload/removal leaves no stale converter behind.

use std::sync::Arc;

use codeterm_blueprint_types::sentinel_input::MediaKind;

use super::tests::{test_caps, TestRegistrar};
use super::*;
use crate::converter::{ConvertOutput, ConvertRequest, Step};
use crate::js_runtime::PluginVm;

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn bump_mtime(path: &std::path::Path) {
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(later)
        .unwrap();
}

fn manifest_with(id: &str, capabilities: &str) -> String {
    use crate::js_runtime::host_api::HOST_API_VERSION;
    format!(
        r#"{{"id":"{id}","version":"0","displayName":"{id}","hostApi":{HOST_API_VERSION},
            "capabilities":{{{capabilities}}},"entry":"plugin.js"}}"#
    )
}

fn request(from: MediaKind, to: MediaKind, dir: &std::path::Path) -> ConvertRequest {
    ConvertRequest {
        input_path: dir.join("in.bin"),
        from,
        to,
        output_dir: dir.to_path_buf(),
        language: None,
        deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
        cancellation: Default::default(),
    }
}

#[test]
fn converter_registry_registers_graph_node_converters_and_deregisters() {
    let caps = test_caps();
    let manifest: PluginManifest = serde_json::from_str(&manifest_with(
        "convert-fixture",
        r#""graphNodes":{"converters":[{"from":"video","to":"audio"}]}"#,
    ))
    .expect("manifest");
    let vm = Arc::new(
        PluginVm::from_source(
            "convert-fixture",
            r#"module.exports.default = {
                convert(request) { return { text: request.from + "->" + request.to }; }
            };"#
            .into(),
        )
        .expect("vm"),
    );

    caps.register_plugin(&manifest, &vm);
    let plan = caps
        .converter
        .plan(MediaKind::Video, MediaKind::Audio)
        .expect("declared conversion is planned");
    let tmp = tempfile::tempdir().unwrap();
    match &plan[0] {
        Step::Convert { key, .. } => match key
            .convert(&request(MediaKind::Video, MediaKind::Audio, tmp.path()))
            .expect("convert")
        {
            ConvertOutput::Text(text) => assert_eq!(text, "video->audio"),
            other => panic!("expected text, got {other:?}"),
        },
        other => panic!("expected a conversion step, got {other:?}"),
    }
    assert!(
        caps.converter
            .plan(MediaKind::Audio, MediaKind::Text)
            .is_err(),
        "undeclared pairs stay unplannable"
    );

    caps.deregister_plugin("convert-fixture");
    assert!(caps
        .converter
        .plan(MediaKind::Video, MediaKind::Audio)
        .is_err());
}

#[test]
fn transcriber_capability_registers_the_audio_text_converter() {
    let caps = test_caps();
    let manifest: PluginManifest =
        serde_json::from_str(&manifest_with("voice-fixture", r#""transcriber":true"#))
            .expect("manifest");
    let vm = Arc::new(
        PluginVm::from_source(
            "voice-fixture",
            r#"module.exports.default = {
                transcribe(path, lang) { return { text: "heard " + lang }; }
            };"#
            .into(),
        )
        .expect("vm"),
    );

    caps.register_plugin(&manifest, &vm);
    let plan = caps
        .converter
        .plan(MediaKind::Audio, MediaKind::Text)
        .expect("a transcription plugin is the audio -> text edge");
    assert_eq!(plan.len(), 1);
    match &plan[0] {
        Step::Convert { key, conversion } => {
            assert_eq!(
                *conversion,
                crate::converter::Conversion::new(MediaKind::Audio, MediaKind::Text)
            );
            let tmp = tempfile::tempdir().unwrap();
            match key
                .convert(&request(MediaKind::Audio, MediaKind::Text, tmp.path()))
                .expect("convert")
            {
                ConvertOutput::Text(text) => assert_eq!(text, "heard"),
                other => panic!("expected text, got {other:?}"),
            }
        }
        other => panic!("expected a conversion step, got {other:?}"),
    }

    caps.deregister_plugin("voice-fixture");
    assert!(caps
        .converter
        .plan(MediaKind::Audio, MediaKind::Text)
        .is_err());
}

#[test]
fn reload_replaces_stale_converters() {
    let root = tempfile::tempdir().unwrap();
    let caps = test_caps();
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        TestRegistrar::new(),
        caps.clone(),
    );
    let dir = root.path().join("shifting");
    write(
        &dir.join("plugin.json"),
        &manifest_with(
            "shifting",
            r#""graphNodes":{"converters":[{"from":"video","to":"audio"}]}"#,
        ),
    );
    write(
        &dir.join("plugin.js"),
        r#"module.exports.default = {
            convert(request) { return { text: "v1:" + request.to }; }
        };"#,
    );
    watcher.tick();
    assert!(caps
        .converter
        .plan(MediaKind::Video, MediaKind::Audio)
        .is_ok());

    write(
        &dir.join("plugin.json"),
        &manifest_with(
            "shifting",
            r#""graphNodes":{"converters":[{"from":"image","to":"text"}]}"#,
        ),
    );
    write(
        &dir.join("plugin.js"),
        r#"module.exports.default = {
            convert(request) { return { text: "v2:" + request.to }; }
        };"#,
    );
    bump_mtime(&dir.join("plugin.js"));
    bump_mtime(&dir.join("plugin.json"));
    watcher.tick();

    assert!(
        caps.converter
            .plan(MediaKind::Video, MediaKind::Audio)
            .is_err(),
        "the stale edge leaves with the reload"
    );
    let plan = caps
        .converter
        .plan(MediaKind::Image, MediaKind::Text)
        .expect("the reloaded edge is planned");
    assert_eq!(plan.len(), 1);

    std::fs::remove_dir_all(&dir).unwrap();
    watcher.tick();
    assert!(
        caps.converter
            .plan(MediaKind::Image, MediaKind::Text)
            .is_err(),
        "folder removal leaves no stale converter"
    );
    assert_eq!(
        caps.converter.converters_for("shifting").len(),
        0,
        "removal clears the id's whole converter entry"
    );
}

#[test]
fn event_sources_alone_register_no_converter() {
    let caps = test_caps();
    let manifest: PluginManifest = serde_json::from_str(&manifest_with(
        "events-fixture",
        r#""graphNodes":{"eventSources":[{"kind":"fixture.event","title":"Fixture"}]}"#,
    ))
    .expect("manifest");
    let vm = Arc::new(
        PluginVm::from_source("events-fixture", "module.exports.default = {};".into()).expect("vm"),
    );

    caps.register_plugin(&manifest, &vm);
    assert!(caps.converter.converters_for("events-fixture").is_empty());
    caps.deregister_plugin("events-fixture");
}

#[test]
fn event_source_registry_refuses_plugin_without_graph_events_grant() {
    let caps = test_caps();
    let manifest: PluginManifest = serde_json::from_str(&manifest_with(
        "ungranted-events",
        r#""graphNodes":{"eventSources":[{"kind":"fixture.event","title":"Fixture"}]}"#,
    ))
    .expect("manifest");
    let sandbox = Arc::new(
        crate::js_runtime::host_api::PluginSandbox::restricted(vec![], vec![])
            .with_identity_and_perms(
                "ungranted-events",
                &crate::js_runtime::manifest::PermissionsConfig::default(),
            ),
    );
    let vm = Arc::new(
        PluginVm::from_source_sandboxed(
            "ungranted-events",
            "module.exports.default = {};".into(),
            None,
            sandbox,
        )
        .expect("vm"),
    );

    caps.register_plugin(&manifest, &vm);

    assert!(caps.event_source.select_provider("fixture.event").is_none());
}
