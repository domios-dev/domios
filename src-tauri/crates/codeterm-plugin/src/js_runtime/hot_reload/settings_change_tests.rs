use super::tests::{set_mtime, spawnable_chatbackend_manifest, test_caps, tick_until, write};
use super::*;

#[derive(Default)]
struct SettingsRecorder {
    settings_changed: Mutex<Vec<String>>,
}

impl PluginProviderRegistrar for SettingsRecorder {
    fn register_provider(&self, _provider: Box<dyn AgentProvider>) {}
    fn unregister_provider(&self, _id: &str) -> bool {
        false
    }
    fn native_fallback(&self, _id: &str) -> Option<Arc<dyn AgentProvider>> {
        None
    }
    fn provider_settings_changed(&self, id: &str) {
        self.settings_changed.lock().push(id.to_string());
    }
}

const PLUGIN_ID: &str = "settings-catalog-q9";

fn first_model(caps: &CapabilityRegistries) -> Option<String> {
    caps.chatbackend
        .descriptor(PLUGIN_ID)
        .and_then(|descriptor| descriptor.available_models.first().cloned())
        .map(|model| model.id)
}

#[test]
fn config_edit_refreshes_the_catalog_and_notifies_without_a_vm_rebuild() {
    let root = tempfile::tempdir().unwrap();
    let caps = test_caps();
    let registrar = Arc::new(SettingsRecorder::default());
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registrar.clone(),
        caps.clone(),
    );
    let dir = root.path().join(PLUGIN_ID);
    write(
        &dir.join("plugin.json"),
        &spawnable_chatbackend_manifest(PLUGIN_ID),
    );
    write(
        &dir.join("plugin.js"),
        r#"module.exports.default = {
            listModels() {
                const model = (JSON.parse(host.settingsJson()) || {}).model || "none";
                return [{ id: model, displayName: model, available: true }];
            }
        };"#,
    );
    let config = dir.join("config.yaml");
    write(&config, "model: alpha\n");

    assert!(tick_until(&watcher, || first_model(&caps).as_deref() == Some("alpha")));
    assert_eq!(watcher.load_count(), 1);
    assert!(registrar.settings_changed.lock().is_empty());

    let prior = std::fs::metadata(&config).unwrap().modified().unwrap();
    write(&config, "model: beta\n");
    set_mtime(&config, prior + Duration::from_secs(2));

    assert!(
        tick_until(&watcher, || first_model(&caps).as_deref() == Some("beta")),
        "a config-only edit must re-resolve the catalog from the new settings"
    );
    assert_eq!(watcher.load_count(), 1, "config-only edit must not rebuild");
    assert_eq!(
        *registrar.settings_changed.lock(),
        vec![PLUGIN_ID.to_string()]
    );
}
