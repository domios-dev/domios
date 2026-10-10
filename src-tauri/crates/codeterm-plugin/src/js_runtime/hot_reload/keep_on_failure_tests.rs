use super::tests::{
    bump_mtime, manifest_for, plugin_for, set_mtime, test_caps, tick_until, write, TestRegistrar,
};
use super::*;
use std::time::{Duration, SystemTime};

fn with_unknown_field(manifest: &str) -> String {
    manifest.replacen('{', r#"{"futureSchemaField":true,"#, 1)
}

fn error_for(watcher: &HotReloadRegistry, id: &str) -> Option<PluginLoadError> {
    watcher
        .load_errors()
        .into_iter()
        .find(|error| error.id == id)
}

#[test]
fn a_registered_plugin_survives_an_unparsable_manifest_until_its_folder_is_removed() {
    let root = tempfile::tempdir().unwrap();
    let registrar = TestRegistrar::new();
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registrar.clone(),
        test_caps(),
    );
    let dir = root.path().join("provider-keeper");
    let manifest_path = dir.join("plugin.json");
    write(&manifest_path, &manifest_for("keeper"));
    write(&dir.join("plugin.js"), &plugin_for("keeper"));
    watcher.tick();
    assert_eq!(registrar.provider_count(), 1);
    assert!(watcher.load_errors().is_empty());

    write(&manifest_path, &with_unknown_field(&manifest_for("keeper")));
    bump_mtime(&manifest_path);
    for _ in 0..3 {
        watcher.tick();
    }
    assert_eq!(
        registrar.provider_count(),
        1,
        "a parse failure must not unregister the provider"
    );
    assert_eq!(watcher.tracked_ids(), vec!["keeper".to_string()]);
    let error = error_for(&watcher, "keeper").expect("parse failure is recorded per plugin");
    assert_eq!(error.path, manifest_path);
    assert!(
        error.message.contains("futureSchemaField"),
        "{}",
        error.message
    );
    assert!(error.kept_previous);
    assert_eq!(watcher.load_count(), 1);

    write(&manifest_path, &manifest_for("keeper"));
    set_mtime(&manifest_path, SystemTime::now() + Duration::from_secs(60));
    watcher.tick();
    assert_eq!(watcher.load_count(), 2, "a fixed manifest reloads");
    assert!(
        watcher.load_errors().is_empty(),
        "a successful reload clears the error"
    );
    assert_eq!(registrar.provider_count(), 1);

    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        tick_until(&watcher, || watcher.tracked_ids().is_empty()
            && registrar.provider_count() == 0),
        "folder removal still unregisters"
    );
}

#[test]
fn a_registered_plugin_survives_an_entry_that_stops_loading() {
    let root = tempfile::tempdir().unwrap();
    let registrar = TestRegistrar::new();
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registrar.clone(),
        test_caps(),
    );
    let dir = root.path().join("sturdy");
    let entry_path = dir.join("plugin.js");
    write(&dir.join("plugin.json"), &manifest_for("sturdy"));
    write(&entry_path, &plugin_for("sturdy"));
    watcher.tick();
    assert_eq!(registrar.provider_count(), 1);

    write(&entry_path, "module.exports.default = {");
    bump_mtime(&entry_path);
    for _ in 0..3 {
        watcher.tick();
    }
    assert_eq!(registrar.provider_count(), 1);
    assert!(error_for(&watcher, "sturdy").is_some());

    write(&entry_path, &plugin_for("sturdy"));
    set_mtime(&entry_path, SystemTime::now() + Duration::from_secs(60));
    watcher.tick();
    assert_eq!(watcher.load_count(), 2);
    assert!(watcher.load_errors().is_empty());
}

#[test]
fn a_never_registered_unparsable_plugin_stays_unregistered_with_a_visible_error() {
    let root = tempfile::tempdir().unwrap();
    let registrar = TestRegistrar::new();
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registrar.clone(),
        test_caps(),
    );
    let dir = root.path().join("provider-fresh");
    write(
        &dir.join("plugin.json"),
        &with_unknown_field(&manifest_for("fresh")),
    );
    write(&dir.join("plugin.js"), &plugin_for("fresh"));
    watcher.tick();
    assert_eq!(registrar.provider_count(), 0);
    assert!(watcher.tracked_ids().is_empty());
    let error = error_for(&watcher, "fresh")
        .expect("the raw manifest id keys the error so the plugin list can show it");
    assert!(!error.kept_previous);

    std::fs::remove_dir_all(&dir).unwrap();
    watcher.tick();
    assert!(watcher.load_errors().is_empty());
}
