use super::tests::{test_caps, TestRegistrar};
use super::*;

struct FixtureLifecycle(Arc<HotReloadRegistry>);

impl codeterm_plugins::InstallLifecycle for FixtureLifecycle {
    fn install(
        &self,
        id: &str,
        destination: &std::path::Path,
        operation: &mut codeterm_plugins::InstallOperation<'_>,
    ) -> Result<(), String> {
        self.0.install(id, destination, |reload| operation(reload))
    }
}

fn write_install_bundle(directory: &std::path::Path, id: &str, version: &str, marker: &str) {
    std::fs::create_dir_all(directory).unwrap();
    std::fs::write(directory.join("plugin.json"), serde_json::json!({
        "id": id, "version": version, "displayName": "Fixture", "entry": "plugin.js",
        "capabilities": {"agentProvider": true}, "hostApi": super::super::host_api::HOST_API_VERSION,
    }).to_string()).unwrap();
    std::fs::write(directory.join("plugin.js"), format!(
        "module.exports.default = {{ marker() {{ return '{marker}'; }}, hold(db) {{ return host.fs.sqliteQuery(db, 'SELECT value FROM items', []); }} }};"
    )).unwrap();
}

#[test]
fn live_vm_releases_windows_bundle_handles_and_rolls_back_failed_registration() {
    let root = tempfile::tempdir().unwrap();
    let plugins = root.path().join("plugins");
    let id = "live-install-fixture";
    let directory = plugins.join(id);
    write_install_bundle(&directory, id, "1.0.0", "old");
    let db = directory.join("fixture.sqlite");
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute_batch("CREATE TABLE items (value TEXT); INSERT INTO items VALUES ('held');")
        .unwrap();
    drop(connection);
    let registrar = TestRegistrar::new();
    let watcher = HotReloadRegistry::new(vec![plugins.clone()], registrar.clone(), test_caps());
    watcher.tick();
    let old_vm = watcher.existing_vm(id).unwrap();
    let _lease = crate::js_runtime::lease_plugin_sqlite(id);
    let rows: serde_json::Value =
        crate::js_runtime::call_json(&old_vm, "hold", &[serde_json::json!(db)]).unwrap();
    assert_eq!(rows, serde_json::json!([{"value": "held"}]));
    #[cfg(windows)]
    assert!(matches!(
        std::fs::rename(&directory, root.path().join("locked"))
            .unwrap_err()
            .raw_os_error(),
        Some(5 | 32)
    ));
    let source = root.path().join("source");
    write_install_bundle(&source, id, "2.0.0", "new");
    let install = || {
        codeterm_plugins::with_install_lifecycle(
            Arc::new(FixtureLifecycle(watcher.clone())),
            || {
                codeterm_plugins::run(
                    codeterm_plugins::PluginCtx {
                        data_root: root.path().into(),
                        version: None,
                    },
                    codeterm_plugins::PluginAction::Install {
                        id: Some(id.into()),
                        from_dir: Some(source.display().to_string()),
                        id_override: None,
                        channel: None,
                    },
                )
            },
        )
    };
    install().unwrap();
    let current_vm = watcher.existing_vm(id).unwrap();
    assert_eq!(
        crate::js_runtime::call_json::<String>(&current_vm, "marker", &[]).unwrap(),
        "new"
    );
    assert_eq!(
        crate::js_runtime::call_json::<String>(&old_vm, "marker", &[]).unwrap(),
        "old"
    );
    assert_eq!(registrar.provider_count(), 1);
    assert_eq!(std::fs::read_dir(&plugins).unwrap().count(), 1);

    write_install_bundle(&source, id, "3.0.0", "broken");
    std::fs::write(source.join("plugin.js"), "not valid JavaScript {{{").unwrap();
    let error = install().unwrap_err();
    assert!(error.contains("previous plugin restored"), "{error}");
    let restored_vm = watcher.existing_vm(id).unwrap();
    assert_eq!(
        crate::js_runtime::call_json::<String>(&restored_vm, "marker", &[]).unwrap(),
        "new"
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(directory.join("plugin.json")).unwrap()
        )
        .unwrap()["version"],
        "2.0.0"
    );
    assert_eq!(registrar.provider_count(), 1);
    assert_eq!(std::fs::read_dir(&plugins).unwrap().count(), 1);
}

#[test]
fn staging_and_rollback_are_never_discovered_as_plugins() {
    let root = tempfile::tempdir().unwrap();
    for name in ["fixture.tmp-install", "fixture.rollback.tmp-install"] {
        let directory = root.path().join(name);
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(
            directory.join("plugin.json"),
            r#"{"id":"fixture","version":"2","displayName":"Fixture"}"#,
        )
        .unwrap();
    }
    let watcher =
        HotReloadRegistry::new(vec![root.path().into()], TestRegistrar::new(), test_caps());
    assert!(watcher.discover_candidates().is_empty());
    let (providers, report) = super::super::loader::scan_roots(&[root.path().into()]);
    assert!(providers.is_empty());
    assert!(report.is_empty());
}

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
