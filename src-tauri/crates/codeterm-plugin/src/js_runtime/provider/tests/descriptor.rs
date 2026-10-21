use super::*;
use crate::js_runtime::call_gate;
use std::sync::{mpsc, Arc};

#[test]
fn descriptor_does_not_wait_for_model_discovery_in_the_vm() {
    let entered = "provider-descriptor-models-entered";
    let release = "provider-descriptor-models-release";
    call_gate::reset(entered);
    call_gate::reset(release);
    let provider = Arc::new(load(
        MIN_MANIFEST,
        &format!(
            r#"module.exports.default = {{
                models() {{
                    host.testGateOpen({entered:?});
                    host.testGateBlock({release:?});
                    return [];
                }},
                authStatusProbe() {{ return null; }}
            }};"#
        ),
    ));
    let vm = provider.vm().clone();
    let discovery = std::thread::spawn(move || call_json::<Value>(&vm, "models", &[]));
    let started = call_gate::wait_opened(entered, Duration::from_secs(3));
    if !started {
        call_gate::open(release);
    }
    assert!(started);
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || tx.send(provider.describe()).unwrap());
    let result = rx.recv_timeout(Duration::from_secs(1));
    call_gate::open(release);
    discovery.join().unwrap().unwrap();
    reader.join().unwrap();
    assert_eq!(result.expect("descriptor waited for models()").id, "demo");
}

#[test]
fn descriptor_refreshes_metadata_explicitly_and_reads_current_catalogue() {
    let manifest = MIN_MANIFEST.replace("\"demo\"", "\"descriptor-refresh\"");
    let mut provider = load(&manifest, "module.exports.default = {};");
    let before = provider.describe();
    provider.manifest.spawn.supports_structured_prompts = true;
    assert!(!provider.describe().supports_structured_prompts);
    provider.refresh_descriptor();
    assert!(provider.describe().supports_structured_prompts);
    crate::model_catalog::put_live(
        provider.id(),
        vec![ModelInfo {
            id: "discovered".into(),
            display_name: "Discovered".into(),
            ..ModelInfo::default()
        }],
        Some("discovered".into()),
    );
    let after = provider.describe();
    assert_eq!(after.available_models[0].id, "discovered");
    assert_eq!(after.default_model.as_deref(), Some("discovered"));
    assert!(after.catalogue_version > before.catalogue_version);
    assert_eq!(
        after.settings_schema.len(),
        before.settings_schema.len() + 1
    );
}
