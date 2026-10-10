use super::tests::{test_caps, TestRegistrar};
use super::*;

#[test]
fn install_acknowledges_registration_without_a_watcher_tick() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("fixture");
    std::fs::create_dir_all(&directory).unwrap();
    let registrar = TestRegistrar::new();
    let watcher = HotReloadRegistry::new(vec![root.path().into()], registrar.clone(), test_caps());
    let manifest = serde_json::json!({
        "id": "fixture", "version": "1", "displayName": "Fixture",
        "capabilities": {"agentProvider": true}, "entry": "plugin.js",
        "hostApi": super::super::host_api::HOST_API_VERSION
    });
    std::fs::write(directory.join("plugin.json"), manifest.to_string()).unwrap();
    std::fs::write(directory.join("plugin.js"), "module.exports.default = {};").unwrap();
    assert_eq!(registrar.provider_count(), 0);
    watcher.refresh_installed(&["fixture".into()]).unwrap();
    assert_eq!(registrar.provider_count(), 1);
    assert_eq!(watcher.tracked_ids(), vec!["fixture"]);
    assert!(watcher.refresh_installed(&["absent".into()]).is_err());
}
