use super::*;

#[test]
fn an_invoked_effect_error_is_never_reported_as_an_absent_export() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("written-once.txt");
    let manifest: PluginManifest = serde_json::from_value(json!({"id":"io-result-refusal","version":"0.0.0","displayName":"Fixture","hostIoJobs":true})).unwrap();
    let vm = Arc::new(PluginVm::from_source_with(
        &manifest.id,
        "module.exports={mutate(path){return host.io.run((function*(){yield* host.io.fs.writeFile(path,'own-effect');throw new Error('after effect');})());}};".into(),
        Some(serde_json::to_string(&manifest).unwrap()),
    ).unwrap());
    let provider = JsProvider::new(manifest, vm);
    assert!(
        matches!(provider.try_call_result("mutate", &[json!(path)]), Some(Err(message)) if message.contains("after effect"))
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), "own-effect");
    assert!(provider.try_call_result("absent", &[]).is_none());
}
