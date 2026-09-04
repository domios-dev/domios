use super::*;
use std::time::{Duration, SystemTime};

use crate::host_traits::PluginProviderRegistrar;
use crate::provider::AgentProvider;

struct TestRegistrar {
    providers: parking_lot::Mutex<Vec<(String, Box<dyn AgentProvider>)>>,
}

impl TestRegistrar {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            providers: parking_lot::Mutex::new(Vec::new()),
        })
    }
    fn provider_count(&self) -> usize {
        self.providers.lock().len()
    }
    fn with_provider<F: FnOnce(&dyn AgentProvider) -> bool>(&self, id: &str, f: F) -> Option<bool> {
        let guard = self.providers.lock();
        guard
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, p)| f(p.as_ref()))
    }
}

impl PluginProviderRegistrar for TestRegistrar {
    fn register_provider(&self, provider: Box<dyn AgentProvider>) {
        let id = provider.id().to_string();
        let mut guard = self.providers.lock();
        guard.retain(|(k, _)| k != &id);
        guard.push((id, provider));
    }
    fn unregister_provider(&self, id: &str) -> bool {
        let mut guard = self.providers.lock();
        let before = guard.len();
        guard.retain(|(k, _)| k != id);
        guard.len() < before
    }
    fn native_fallback(&self, _id: &str) -> Option<std::sync::Arc<dyn AgentProvider>> {
        None
    }
}

fn test_caps() -> CapabilityRegistries {
    let capabilities = CapabilityRegistries {
        transcribe: Arc::new(crate::transcribe::TranscribeRegistry::new()),
        glance: Arc::new(crate::glance::GlanceRegistry::new()),
        bubble: Arc::new(crate::bubble::BubbleRegistry::new()),
        view: Arc::new(crate::view_host::ViewRegistry::new()),
        chatbackend: Arc::new(crate::chatbackend_provider::ChatBackendRegistry::new()),
        chat_preprocessor: Arc::new(crate::chatpreprocessor::ChatPreprocessorRegistry::new()),
        agent_command: Arc::new(crate::agent_commands::AgentCommandRegistry::new()),
        secret: None,
    };
    let _ = capabilities.chatbackend.start_discovery();
    capabilities
}

// Poll tick() until `condition` holds instead of assuming one tick is enough;
// bounded so a genuine regression still fails instead of hanging.
fn tick_until<F: Fn() -> bool>(watcher: &HotReloadRegistry, condition: F) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        watcher.tick();
        if condition() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

// Windows refuses to stamp file times through a read-only handle, so open for
// write rather than File::open.
fn set_mtime(path: &std::path::Path, mtime: std::time::SystemTime) {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
}

fn manifest_for(id: &str) -> String {
    use crate::js_runtime::host_api::HOST_API_VERSION;
    format!(
        r#"{{"id":"{id}","version":"0","displayName":"{id}","hostApi":{HOST_API_VERSION},
            "spawn":{{"readinessStrategy":{{"kind":"fixed_delay","durationMs":0}},
                     "taskDelivery":"pty_input","verifySubmit":{{"kind":"none"}}}},
            "entry":"plugin.js"}}"#
    )
}

fn capability_manifest_for(id: &str, cap_field: &str) -> String {
    capability_manifest_full(id, cap_field, false)
}

fn capability_manifest_full(id: &str, cap_field: &str, requires_reload: bool) -> String {
    use crate::js_runtime::host_api::HOST_API_VERSION;
    format!(
        r#"{{"id":"{id}","version":"0","displayName":"{id}","hostApi":{HOST_API_VERSION},
            "spawn":{{"readinessStrategy":{{"kind":"fixed_delay","durationMs":0}},
                     "taskDelivery":"pty_input","verifySubmit":{{"kind":"none"}}}},
            "capabilities":{{"{cap_field}":true}},"requiresReload":{requires_reload},
            "entry":"plugin.js"}}"#
    )
}

fn spawnable_chatbackend_manifest(id: &str) -> String {
    use crate::js_runtime::host_api::HOST_API_VERSION;
    format!(
        r#"{{"id":"{id}","version":"0","displayName":"{id}","hostApi":{HOST_API_VERSION},
            "capabilities":{{"chatBackend":{{"spawnable":true}}}},"entry":"plugin.js"}}"#
    )
}

fn capabilityless_manifest(id: &str) -> String {
    use crate::js_runtime::host_api::HOST_API_VERSION;
    format!(
        r#"{{"id":"{id}","version":"0","displayName":"{id}","hostApi":{HOST_API_VERSION},
            "capabilities":{{}},"entry":"plugin.js"}}"#
    )
}

fn chatbackend_with_model(model: &str) -> String {
    format!(
        r#"module.exports.default = {{
            listModels() {{ return [{{ id: "{model}", displayName: "{model}", available: true }}]; }}
        }};"#
    )
}

fn plugin_for(id: &str) -> String {
    format!(
        r#"module.exports.default = {{ detectFromTitle: function(t) {{ return t.indexOf("{id}") !== -1; }} }};"#
    )
}

#[test]
fn watcher_holds_threaded_capability_registries() {
    let root = tempfile::tempdir().unwrap();
    let registrar = TestRegistrar::new();
    let caps = test_caps();
    let watcher = HotReloadRegistry::new(vec![root.path().to_path_buf()], registrar, caps.clone());
    let held = watcher.capability_registries();
    assert!(Arc::ptr_eq(&held.transcribe, &caps.transcribe));
    assert!(Arc::ptr_eq(&held.glance, &caps.glance));
    assert!(Arc::ptr_eq(&held.bubble, &caps.bubble));
    assert!(Arc::ptr_eq(&held.view, &caps.view));
    assert!(Arc::ptr_eq(&held.chatbackend, &caps.chatbackend));
    assert!(Arc::ptr_eq(
        &held.chat_preprocessor,
        &caps.chat_preprocessor
    ));
}

#[test]
fn watcher_bind_service_noop_without_ref() {
    let watcher = HotReloadRegistry::new(vec![], TestRegistrar::new(), test_caps());
    let vm = Arc::new(
        crate::js_runtime::PluginVm::from_source("p", "module.exports.default = {};".into())
            .unwrap(),
    );
    assert!(
        !watcher.bind_service(&vm),
        "no service ref set → nothing to bind"
    );
}

#[test]
fn requires_reload_plugin_installed_live_is_pending_not_active() {
    let root = tempfile::tempdir().unwrap();
    let registrar = TestRegistrar::new();
    let caps = test_caps();
    let watcher = HotReloadRegistry::new(vec![root.path().to_path_buf()], registrar, caps.clone());
    let dir = root.path().join("needsreload");
    write(
        &dir.join("plugin.json"),
        &capability_manifest_full("needsreload", "statusBubble", true),
    );
    write(&dir.join("plugin.js"), "module.exports.default = {};");
    watcher.tick();
    assert!(
        !caps.bubble.contains("needsreload"),
        "requiresReload must NOT hot-activate"
    );
    assert!(
        watcher
            .pending_reload_ids()
            .contains(&"needsreload".to_string()),
        "fresh requiresReload install must be pending_reload"
    );
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        tick_until(&watcher, || !watcher
            .pending_reload_ids()
            .contains(&"needsreload".to_string())),
        "folder removal must clear pending_reload"
    );
}

#[test]
fn seeded_requires_reload_plugin_is_not_pending() {
    let root = tempfile::tempdir().unwrap();
    let registrar = TestRegistrar::new();
    let caps = test_caps();
    let watcher = HotReloadRegistry::new(vec![root.path().to_path_buf()], registrar, caps.clone());
    let dir = root.path().join("startupreload");
    write(
        &dir.join("plugin.json"),
        &capability_manifest_full("startupreload", "statusBubble", true),
    );
    write(&dir.join("plugin.js"), "module.exports.default = {};");
    watcher.seed();
    assert!(!watcher
        .pending_reload_ids()
        .contains(&"startupreload".to_string()));
    std::thread::sleep(Duration::from_millis(1100));
    write(&dir.join("plugin.js"), "module.exports.default = { v: 2 };");
    watcher.tick();
    assert!(!watcher
        .pending_reload_ids()
        .contains(&"startupreload".to_string()));
}

#[test]
fn tick_registers_new_plugins_added_after_start() {
    let root = tempfile::tempdir().unwrap();
    let registrar = TestRegistrar::new();
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registrar.clone(),
        test_caps(),
    );
    watcher.tick();
    assert!(watcher.tracked_ids().is_empty());
    assert_eq!(registrar.provider_count(), 0);

    let dir = root.path().join("foo");
    write(&dir.join("plugin.json"), &manifest_for("foo"));
    write(&dir.join("plugin.js"), &plugin_for("foo"));
    watcher.tick();
    assert_eq!(watcher.tracked_ids(), vec!["foo".to_string()]);
    assert_eq!(registrar.provider_count(), 1);
    assert!(registrar
        .with_provider("foo", |p| p.detect_from_title("foo cli"))
        .unwrap_or(false));
}

#[test]
fn hot_reload_reaches_provider_integration_refresh_without_restart() {
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    env.set("CODETERM_API_PORT", "7686");
    let root = tempfile::tempdir().unwrap();
    let registrar = TestRegistrar::new();
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registrar.clone(),
        test_caps(),
    );
    let dir = root.path().join("refreshable");
    write(&dir.join("plugin.json"), &manifest_for("refreshable"));
    write(
        &dir.join("plugin.js"),
        "module.exports.default = { ensureIntegrationOnStartup() {}, detectFromTitle() { return false; } };",
    );
    watcher.tick();
    assert!(!registrar
        .with_provider("refreshable", |p| p.detect_from_title("refresh"))
        .unwrap_or(true));

    write(
        &dir.join("plugin.js"),
        "let refreshed = false; module.exports.default = { ensureIntegrationOnStartup() { refreshed = true; }, detectFromTitle() { return refreshed; } };",
    );
    set_mtime(
        &dir.join("plugin.js"),
        SystemTime::now() + Duration::from_secs(1),
    );
    watcher.tick();

    assert!(
        registrar
            .with_provider("refreshable", |p| p.detect_from_title("refresh"))
            .unwrap_or(false),
        "hot reload must call the provider refresh seam"
    );
}

#[test]
fn tick_hot_reloads_plugin_js_changes() {
    let root = tempfile::tempdir().unwrap();
    let registrar = TestRegistrar::new();
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registrar.clone(),
        test_caps(),
    );
    let dir = root.path().join("bar");
    write(&dir.join("plugin.json"), &manifest_for("bar"));
    write(&dir.join("plugin.js"), &plugin_for("v1"));
    watcher.tick();
    assert!(registrar
        .with_provider("bar", |p| p.detect_from_title("v1"))
        .unwrap_or(false));
    assert!(!registrar
        .with_provider("bar", |p| p.detect_from_title("v2"))
        .unwrap_or(true));

    std::thread::sleep(Duration::from_millis(1100));
    write(&dir.join("plugin.js"), &plugin_for("v2"));
    watcher.tick();

    assert!(registrar
        .with_provider("bar", |p| p.detect_from_title("v2"))
        .unwrap_or(false));
    assert!(!registrar
        .with_provider("bar", |p| p.detect_from_title("v1"))
        .unwrap_or(true));
}

#[test]
fn duplicate_roots_only_load_the_winning_plugin() {
    let workspace = tempfile::tempdir().unwrap();
    let user = tempfile::tempdir().unwrap();
    let registrar = TestRegistrar::new();
    let watcher = HotReloadRegistry::new(
        vec![workspace.path().to_path_buf(), user.path().to_path_buf()],
        registrar.clone(),
        test_caps(),
    );

    let workspace_dir = workspace.path().join("provider-dupe");
    write(&workspace_dir.join("plugin.json"), &manifest_for("dupe"));
    write(&workspace_dir.join("plugin.js"), &plugin_for("workspace"));
    let user_dir = user.path().join("dupe");
    write(&user_dir.join("plugin.json"), &manifest_for("dupe"));
    write(&user_dir.join("plugin.js"), &plugin_for("user"));

    watcher.tick();
    assert_eq!(watcher.load_count(), 1);
    assert!(registrar
        .with_provider("dupe", |p| p.detect_from_title("user cli"))
        .unwrap_or(false));
    assert!(!registrar
        .with_provider("dupe", |p| p.detect_from_title("workspace cli"))
        .unwrap_or(true));

    watcher.tick();
    assert_eq!(
        watcher.load_count(),
        1,
        "unchanged duplicate roots must not ping-pong reload"
    );
}

#[test]
fn duplicate_root_removal_falls_back_to_lower_priority_plugin() {
    let workspace = tempfile::tempdir().unwrap();
    let user = tempfile::tempdir().unwrap();
    let registrar = TestRegistrar::new();
    let watcher = HotReloadRegistry::new(
        vec![workspace.path().to_path_buf(), user.path().to_path_buf()],
        registrar.clone(),
        test_caps(),
    );

    let workspace_dir = workspace.path().join("provider-dupe");
    write(&workspace_dir.join("plugin.json"), &manifest_for("dupe"));
    write(&workspace_dir.join("plugin.js"), &plugin_for("workspace"));
    let user_dir = user.path().join("dupe");
    write(&user_dir.join("plugin.json"), &manifest_for("dupe"));
    write(&user_dir.join("plugin.js"), &plugin_for("user"));

    watcher.tick();
    assert!(registrar
        .with_provider("dupe", |p| p.detect_from_title("user cli"))
        .unwrap_or(false));

    std::fs::remove_dir_all(&user_dir).unwrap();
    watcher.tick();

    assert!(registrar
        .with_provider("dupe", |p| p.detect_from_title("workspace cli"))
        .unwrap_or(false));
    assert!(!registrar
        .with_provider("dupe", |p| p.detect_from_title("user cli"))
        .unwrap_or(true));
}

#[test]
fn duplicate_root_removal_falls_back_even_with_colliding_mtimes() {
    // Windows' coarse system clock can give two back-to-back writes the same
    // mtime; force that collision to pin the fallback-by-path invariant.
    let workspace = tempfile::tempdir().unwrap();
    let user = tempfile::tempdir().unwrap();
    let registrar = TestRegistrar::new();
    let watcher = HotReloadRegistry::new(
        vec![workspace.path().to_path_buf(), user.path().to_path_buf()],
        registrar.clone(),
        test_caps(),
    );

    let workspace_dir = workspace.path().join("provider-dupe");
    write(&workspace_dir.join("plugin.json"), &manifest_for("dupe"));
    write(&workspace_dir.join("plugin.js"), &plugin_for("workspace"));
    let user_dir = user.path().join("dupe");
    write(&user_dir.join("plugin.json"), &manifest_for("dupe"));
    write(&user_dir.join("plugin.js"), &plugin_for("user"));

    let user_manifest_mtime = std::fs::metadata(user_dir.join("plugin.json"))
        .unwrap()
        .modified()
        .unwrap();
    let user_entry_mtime = std::fs::metadata(user_dir.join("plugin.js"))
        .unwrap()
        .modified()
        .unwrap();
    set_mtime(&workspace_dir.join("plugin.json"), user_manifest_mtime);
    set_mtime(&workspace_dir.join("plugin.js"), user_entry_mtime);

    watcher.tick();
    assert!(registrar
        .with_provider("dupe", |p| p.detect_from_title("user cli"))
        .unwrap_or(false));

    std::fs::remove_dir_all(&user_dir).unwrap();
    watcher.tick();
    assert!(
        registrar
            .with_provider("dupe", |p| p.detect_from_title("workspace cli"))
            .unwrap_or(false),
        "fallback must reload even when the winning and fallback candidates share an mtime"
    );
}

#[test]
fn tick_registers_capability_plugin_live_without_restart() {
    let root = tempfile::tempdir().unwrap();
    let registrar = TestRegistrar::new();
    let caps = test_caps();
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registrar.clone(),
        caps.clone(),
    );
    let dir = root.path().join("bubbler");
    write(
        &dir.join("plugin.json"),
        &capability_manifest_for("bubbler", "statusBubble"),
    );
    write(&dir.join("plugin.js"), "module.exports.default = {};");
    watcher.tick();
    assert_eq!(registrar.provider_count(), 0);
    assert!(
        caps.bubble.contains("bubbler"),
        "status bubble must register live on install"
    );
}

#[test]
fn tick_deregisters_capability_on_folder_removal() {
    let root = tempfile::tempdir().unwrap();
    let registrar = TestRegistrar::new();
    let caps = test_caps();
    let watcher = HotReloadRegistry::new(vec![root.path().to_path_buf()], registrar, caps.clone());
    let dir = root.path().join("bubbler");
    write(
        &dir.join("plugin.json"),
        &capability_manifest_for("bubbler", "statusBubble"),
    );
    write(&dir.join("plugin.js"), "module.exports.default = {};");
    watcher.tick();
    assert!(caps.bubble.contains("bubbler"));
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        tick_until(&watcher, || !caps.bubble.contains("bubbler")),
        "capability must be deregistered on uninstall"
    );
}

#[test]
fn chatbackend_model_refresh_is_provider_scoped_and_event_driven() {
    let root = tempfile::tempdir().unwrap();
    let caps = test_caps();
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        TestRegistrar::new(),
        caps.clone(),
    );
    let sentinel_dir = root.path().join("sentinel");
    let foreign_dir = root.path().join("foreign");
    write(
        &sentinel_dir.join("plugin.json"),
        &spawnable_chatbackend_manifest("sentinel"),
    );
    write(
        &sentinel_dir.join("plugin.js"),
        &chatbackend_with_model("sentinel-a"),
    );
    write(
        &foreign_dir.join("plugin.json"),
        &spawnable_chatbackend_manifest("foreign"),
    );
    write(
        &foreign_dir.join("plugin.js"),
        &chatbackend_with_model("foreign-model"),
    );

    assert!(
        tick_until(&watcher, || {
            caps.chatbackend
                .descriptor("sentinel")
                .and_then(|descriptor| descriptor.available_models.first().cloned())
                .is_some_and(|model| model.id == "sentinel-a")
        }),
        "initial model discovery must complete"
    );
    assert_eq!(watcher.contribution_event_count(), 2);

    let prior_mtime = std::fs::metadata(sentinel_dir.join("plugin.js"))
        .unwrap()
        .modified()
        .unwrap();
    write(
        &sentinel_dir.join("plugin.js"),
        &chatbackend_with_model("sentinel-b"),
    );
    set_mtime(
        &sentinel_dir.join("plugin.js"),
        prior_mtime + Duration::from_secs(2),
    );
    assert!(
        tick_until(&watcher, || {
            caps.chatbackend
                .descriptor("sentinel")
                .and_then(|descriptor| descriptor.available_models.first().cloned())
                .is_some_and(|model| model.id == "sentinel-b")
        }),
        "reloaded model discovery must complete"
    );
    assert_eq!(
        caps.chatbackend
            .descriptor("foreign")
            .unwrap()
            .available_models[0]
            .id,
        "foreign-model",
        "refresh must not replace a foreign provider snapshot"
    );
    assert_eq!(watcher.contribution_event_count(), 3);

    std::fs::remove_dir_all(&sentinel_dir).unwrap();
    watcher.tick();

    assert!(caps.chatbackend.descriptor("sentinel").is_none());
    assert!(crate::model_catalog::registered_snapshot("sentinel")
        .models
        .is_empty());
    assert_eq!(
        caps.chatbackend
            .descriptor("foreign")
            .unwrap()
            .available_models[0]
            .id,
        "foreign-model"
    );
    assert_eq!(watcher.contribution_event_count(), 4);
}

#[test]
fn chatbackend_disable_deregisters_registry_and_model_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let caps = test_caps();
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        TestRegistrar::new(),
        caps.clone(),
    );
    let dir = root.path().join("disable-chatbackend");
    let manifest_path = dir.join("plugin.json");
    write(
        &manifest_path,
        &spawnable_chatbackend_manifest("disable-chatbackend"),
    );
    write(
        &dir.join("plugin.js"),
        &chatbackend_with_model("stale-model"),
    );
    watcher.tick();
    assert!(caps.chatbackend.contains("disable-chatbackend"));
    assert_eq!(watcher.contribution_event_count(), 1);

    let prior_mtime = std::fs::metadata(&manifest_path)
        .unwrap()
        .modified()
        .unwrap();
    write(
        &manifest_path,
        &capabilityless_manifest("disable-chatbackend"),
    );
    set_mtime(&manifest_path, prior_mtime + Duration::from_secs(2));
    watcher.tick();

    assert!(!caps.chatbackend.contains("disable-chatbackend"));
    assert!(
        crate::model_catalog::registered_snapshot("disable-chatbackend")
            .models
            .is_empty()
    );
    assert_eq!(
        watcher.contribution_event_count(),
        2,
        "disabling a visible chatBackend emits the provider contribution inverse"
    );
}

#[test]
fn chatbackend_spawnable_true_to_false_emits_contribution_inverse() {
    let root = tempfile::tempdir().unwrap();
    let caps = test_caps();
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        TestRegistrar::new(),
        caps.clone(),
    );
    let dir = root.path().join("hide-chatbackend");
    let manifest_path = dir.join("plugin.json");
    write(
        &manifest_path,
        &spawnable_chatbackend_manifest("hide-chatbackend"),
    );
    write(
        &dir.join("plugin.js"),
        &chatbackend_with_model("still-registered"),
    );
    watcher.tick();
    assert!(caps
        .chatbackend
        .get("hide-chatbackend")
        .unwrap()
        .spawnable());
    assert_eq!(watcher.contribution_event_count(), 1);

    let prior_mtime = std::fs::metadata(&manifest_path)
        .unwrap()
        .modified()
        .unwrap();
    write(
        &manifest_path,
        &capability_manifest_for("hide-chatbackend", "chatBackend"),
    );
    set_mtime(&manifest_path, prior_mtime + Duration::from_secs(2));
    watcher.tick();

    let backend = caps.chatbackend.get("hide-chatbackend").unwrap();
    assert!(!backend.spawnable());
    let models = backend.list_models().expect("backend still lists models");
    assert_eq!(
        models.first().map(|model| model.id.as_str()),
        Some("still-registered")
    );
    assert_eq!(
        watcher.contribution_event_count(),
        2,
        "spawnable true-to-false emits the removal-side provider event"
    );
}

#[test]
fn tick_unregisters_plugins_after_folder_removal() {
    let root = tempfile::tempdir().unwrap();
    let registrar = TestRegistrar::new();
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registrar.clone(),
        test_caps(),
    );
    let dir = root.path().join("baz");
    write(&dir.join("plugin.json"), &manifest_for("baz"));
    write(&dir.join("plugin.js"), &plugin_for("baz"));
    watcher.tick();
    assert_eq!(registrar.provider_count(), 1);
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        tick_until(&watcher, || watcher.tracked_ids().is_empty()
            && registrar.provider_count() == 0),
        "folder removal must unregister the plugin"
    );
}

#[test]
fn config_yaml_edit_restamps_settings_without_vm_rebuild() {
    let root = tempfile::tempdir().unwrap();
    let registrar = TestRegistrar::new();
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registrar.clone(),
        test_caps(),
    );
    let dir = root.path().join("cfgplugin");
    write(&dir.join("plugin.json"), &manifest_for("cfgplugin"));
    write(
        &dir.join("plugin.js"),
        r#"module.exports.default = {
                detectFromTitle: function(t) { return false; },
                settings: function() { return JSON.parse(host.settingsJson()); }
            };"#,
    );
    write(&dir.join("config.yaml"), "engine: whisper\n");
    watcher.tick();
    assert_eq!(watcher.load_count(), 1);

    let vm = watcher
        .existing_vm("cfgplugin")
        .expect("VM tracked after load");
    let before: serde_json::Value = super::super::call_json(&vm, "settings", &[]).unwrap();
    assert_eq!(before["engine"], "whisper");

    watcher.tick();
    assert_eq!(watcher.load_count(), 1);

    std::thread::sleep(Duration::from_millis(1100));
    write(&dir.join("config.yaml"), "engine: deepgram\n");
    watcher.tick();
    assert_eq!(
        watcher.load_count(),
        1,
        "config-only edit must NOT rebuild the VM"
    );

    let after: serde_json::Value = super::super::call_json(&vm, "settings", &[]).unwrap();
    assert_eq!(
        after["engine"], "deepgram",
        "re-stamped settings must be live on the existing VM"
    );

    std::thread::sleep(Duration::from_millis(1100));
    write(&dir.join("config.yaml"), "engine: vosk\n");
    watcher.tick();
    assert_eq!(watcher.load_count(), 1);
    let again: serde_json::Value = super::super::call_json(&vm, "settings", &[]).unwrap();
    assert_eq!(again["engine"], "vosk");
}

#[test]
fn concurrent_tick_and_reload_does_not_deadlock() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let root = tempfile::tempdir().unwrap();
    let registrar = TestRegistrar::new();
    let watcher = Arc::new(HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registrar,
        test_caps(),
    ));
    let stop = Arc::new(AtomicBool::new(false));
    let dir = root.path().join("churn");

    let churner = {
        let stop = stop.clone();
        let dir = dir.clone();
        std::thread::spawn(move || {
            for i in 0..40 {
                write(&dir.join("plugin.json"), &manifest_for("churn"));
                write(&dir.join("plugin.js"), &plugin_for(&format!("v{i}")));
                std::fs::remove_dir_all(&dir).ok();
            }
            stop.store(true, Ordering::SeqCst);
        })
    };

    let tickers: Vec<_> = (0..4)
        .map(|_| {
            let watcher = watcher.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    watcher.tick();
                }
                watcher.tick();
            })
        })
        .collect();

    churner.join().unwrap();
    for t in tickers {
        t.join().unwrap();
    }
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        tick_until(&watcher, || watcher.tracked_ids().is_empty()),
        "final removal must clear tracked ids"
    );
}

#[test]
fn reloading_a_view_plugin_announces_it_so_an_open_iframe_can_rebuild() {
    let root = tempfile::tempdir().unwrap();
    let caps = test_caps();
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        TestRegistrar::new(),
        caps.clone(),
    );
    let viewer = root.path().join("viewer");
    let headless = root.path().join("headless");
    write(
        &viewer.join("plugin.json"),
        &capability_manifest_for("viewer", "view"),
    );
    write(&viewer.join("plugin.js"), &plugin_for("viewer"));
    write(
        &headless.join("plugin.json"),
        &capability_manifest_for("headless", "transcriber"),
    );
    write(&headless.join("plugin.js"), &plugin_for("headless"));

    assert!(
        tick_until(&watcher, || caps.view.get("viewer").is_some()),
        "the view plugin must register"
    );
    assert_eq!(watcher.view_reload_events(), vec!["viewer".to_string()]);

    let prior = std::fs::metadata(viewer.join("plugin.js"))
        .unwrap()
        .modified()
        .unwrap();
    write(&viewer.join("plugin.js"), &plugin_for("viewer"));
    set_mtime(&viewer.join("plugin.js"), prior + Duration::from_secs(2));
    assert!(
        tick_until(&watcher, || watcher.view_reload_events().len() == 2),
        "a reloaded view plugin must announce again, or an open iframe keeps the old bundle"
    );

    // A plugin with no view has no iframe to rebuild; announcing would reload
    // every other plugin's view for nothing.
    assert!(
        watcher.view_reload_events().iter().all(|id| id == "viewer"),
        "only view-capable plugins may announce, got {:?}",
        watcher.view_reload_events()
    );
}
