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

#[tokio::test]
async fn live_catalogue_refresh_replaces_the_previous_default_and_excludes_seed_models() {
    use crate::model_catalog;

    let id = "descriptor-live-catalogue-refresh";
    let directory = tempfile::tempdir().unwrap();
    let cache = directory.path().join("models_cache.json");
    let mut manifest: Value = serde_json::from_str(MIN_MANIFEST).unwrap();
    manifest["id"] = json!(id);
    manifest["models"] = json!([
        {"id": "seed-only", "displayName": "Seed only"},
        {"id": "older", "displayName": "Older"}
    ]);
    manifest["defaultModel"] = json!("older");
    manifest["modelCatalogue"] = json!({
        "authority": "authoritative",
        "liveDiscoveryRequired": true,
        "configFiles": [cache]
    });
    let provider = Arc::new(load(
        &manifest.to_string(),
        r#"module.exports.default = {
            discoverModels() {
                return [
                    {id:"new-default", displayName:"New default"},
                    {id:"new-alternative", displayName:"New alternative"},
                    {id:"older", displayName:"Older"}
                ];
            }
        };"#,
    ));
    model_catalog::put_live(
        id,
        vec![ModelInfo {
            id: "older".into(),
            display_name: "Older".into(),
            ..ModelInfo::default()
        }],
        Some("older".into()),
    );
    let before = provider.describe();
    assert_eq!(before.default_model.as_deref(), Some("older"));
    std::fs::write(cache, b"updated").unwrap();

    assert!(model_catalog::refresh_models(provider.clone(), false)
        .await
        .is_some());
    let after = provider.describe();
    assert!(after.catalogue_version > before.catalogue_version);
    assert_eq!(after.default_model.as_deref(), Some("new-default"));
    assert_eq!(
        after
            .available_models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        ["new-default", "new-alternative", "older"]
    );
    model_catalog::unregister(id);
    model_catalog::cleanup(id);
}
