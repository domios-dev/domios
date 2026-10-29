use super::*;
use crate::js_runtime::{
    host_api::PluginSandbox,
    vm::{call_json, VmError},
};
use serde_json::{json, Value};

#[test]
fn plugin_identity_refuses_manifest_preconstructed_sandbox_and_vm_before_host_store_access() {
    let root = tempfile::tempdir().unwrap();
    let host = root.path().join("blueprint-plugin-operations.sqlite");
    std::fs::write(&host, b"seeded host authority").unwrap();
    for id in [
        "blueprint-plugin-operations.sqlite",
        "blueprint-run-media.sqlite",
        "../host",
        "C:/host",
        "/host",
        "nul",
        "con",
        "com1",
        "lpt9",
    ] {
        let manifest_json = json!({"id":id,"displayName":"Fixture","version":"0.1.0"}).to_string();
        assert!(PluginManifest::from_json_str(&manifest_json).is_err());
        let manifest: PluginManifest = serde_json::from_str(&manifest_json).unwrap();
        assert!(compute_sandbox(
            &crate::provider::ProviderTrust::ThirdParty,
            root.path(),
            &manifest
        )
        .is_err());
        let source = format!(
            "host.fs.writeFile({},'modified');module.exports.default={{}};",
            json!(host.to_string_lossy())
        );
        let sandbox = Arc::new(PluginSandbox::restricted(
            vec![host.clone()],
            vec![host.clone()],
        ));
        assert!(matches!(
            PluginVm::from_source_sandboxed(id, source, None, sandbox),
            Err(VmError::RuntimeInit(_))
        ));
        assert!(matches!(
            PluginVm::load(id, &root.path().join("not-readable-or-present.js")),
            Err(VmError::RuntimeInit(_))
        ));
        assert_eq!(std::fs::read(&host).unwrap(), b"seeded host authority");
    }
    assert!(PluginVm::from_source_with(
        "valid-plugin",
        "throw Error('must not enter');".into(),
        Some(json!({"id":"blueprint-plugin-operations.sqlite"}).to_string())
    )
    .is_err());
    let missing = root.path().join("source-must-not-be-read.js");
    let forged = json!({"id":"blueprint-plugin-operations.sqlite"}).to_string();
    assert!(matches!(
        PluginVm::load_with_manifest("valid-plugin", &missing, forged.clone()),
        Err(VmError::RuntimeInit(_))
    ));
    let sandbox = Arc::new(PluginSandbox::restricted(
        vec![host.clone()],
        vec![host.clone()],
    ));
    assert!(matches!(
        PluginVm::load_with_manifest_sandboxed("valid-plugin", &missing, forged, sandbox),
        Err(VmError::RuntimeInit(_))
    ));
}

#[test]
fn plugin_identity_valid_confined_vm_retains_only_its_real_directory() {
    let root = tempfile::tempdir().unwrap();
    let owned = root.path().join("valid-plugin");
    std::fs::create_dir(&owned).unwrap();
    let file = owned.join("own.txt");
    let host = root.path().join("blueprint-run-media.sqlite");
    std::fs::write(&host, "seeded host authority").unwrap();
    let source = format!("module.exports.default={{probe(){{return {{own:host.fs.writeFile({},'owned'),read:host.fs.readFile({}),foreign:host.fs.readFile({}),writeForeign:host.fs.writeFile({},'modified')}};}}}};",json!(file.to_string_lossy()),json!(file.to_string_lossy()),json!(host.to_string_lossy()),json!(host.to_string_lossy()));
    let sandbox = Arc::new(
        PluginSandbox::restricted(vec![owned.clone()], vec![owned]).with_identity("valid-plugin"),
    );
    let vm = PluginVm::from_source_sandboxed("valid-plugin", source, None, sandbox).unwrap();
    let result: Value = call_json(&vm, "probe", &[]).unwrap();
    assert_eq!(
        result,
        json!({"own":true,"read":"owned","foreign":null,"writeForeign":false})
    );
    assert_eq!(
        std::fs::read_to_string(host).unwrap(),
        "seeded host authority"
    );
}
