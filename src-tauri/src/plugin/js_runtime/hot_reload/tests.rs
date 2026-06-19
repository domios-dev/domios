use super::*;

fn test_caps() -> CapabilityRegistries {
    CapabilityRegistries {
        secret: Arc::new(crate::secrets::SecretRegistry::new()),
        transcribe: Arc::new(crate::transcribe::TranscribeRegistry::new()),
        glance: Arc::new(crate::glance::GlanceRegistry::new()),
        bubble: Arc::new(crate::bubble::BubbleRegistry::new()),
        view: Arc::new(crate::view_host::ViewRegistry::new()),
        chatbackend: Arc::new(crate::chatbackend::ChatBackendRegistry::new()),
        chat_preprocessor: Arc::new(crate::chatpreprocessor::ChatPreprocessorRegistry::new()),
        agent_command: Arc::new(crate::agent_commands::AgentCommandRegistry::new()),
    }
}

#[test]
fn watcher_holds_threaded_capability_registries() {
    // C0: the watcher must carry the existing capability registry handles so
    // a later track can re-register a (re)loaded plugin's non-agent
    // capabilities live (plan §1.8). Prove the *same* handles are threaded in.
    let root = tempfile::tempdir().unwrap();
    let registry = Arc::new(AgentRegistry::new());
    let caps = test_caps();
    let watcher = HotReloadRegistry::new(vec![root.path().to_path_buf()], registry, caps.clone());
    let held = watcher.capability_registries();
    assert!(Arc::ptr_eq(&held.secret, &caps.secret));
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
fn watcher_binds_service_into_freshly_loaded_vm() {
    // P4: a plugin installed/reloaded AFTER daemon start must get the
    // ServiceContext late-bound into its VM, or host.mem/worker/agent are
    // dead for it — only the startup loop bound the service before this fix.
    use std::sync::Weak;
    let watcher = HotReloadRegistry::new(vec![], Arc::new(AgentRegistry::new()), test_caps());
    watcher.set_service_ref(Weak::new());
    let vm = Arc::new(
        crate::plugin::js_runtime::PluginVm::from_source(
            "p",
            "module.exports.default = {};".into(),
        )
        .unwrap(),
    );
    assert!(
        watcher.bind_service(&vm),
        "watcher binds the service ref into the new VM"
    );
    assert!(
        !vm.set_service(Weak::new()),
        "already bound — the watcher already populated the service ref"
    );
}

#[test]
fn watcher_bind_service_noop_without_ref() {
    let watcher = HotReloadRegistry::new(vec![], Arc::new(AgentRegistry::new()), test_caps());
    let vm = Arc::new(
        crate::plugin::js_runtime::PluginVm::from_source(
            "p",
            "module.exports.default = {};".into(),
        )
        .unwrap(),
    );
    assert!(
        !watcher.bind_service(&vm),
        "no service ref set → nothing to bind"
    );
}

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn manifest_for(id: &str) -> String {
    use crate::plugin::js_runtime::host_api::HOST_API_VERSION;
    format!(
        r#"{{
                "id":"{id}","version":"0","displayName":"{id}","hostApi":{HOST_API_VERSION},
                "spawn":{{"readinessStrategy":{{"kind":"fixed_delay","durationMs":0}},
                         "taskDelivery":"pty_input","verifySubmit":{{"kind":"none"}}}},
                "entry":"plugin.js"
            }}"#
    )
}

/// A capability-only plugin manifest (agent_provider defaults false because a
/// `capabilities` block is present). `requires_reload` toggles the §1.8 flag.
fn capability_manifest_for(id: &str, cap_field: &str) -> String {
    capability_manifest_full(id, cap_field, false)
}

fn capability_manifest_full(id: &str, cap_field: &str, requires_reload: bool) -> String {
    use crate::plugin::js_runtime::host_api::HOST_API_VERSION;
    format!(
        r#"{{
                "id":"{id}","version":"0","displayName":"{id}","hostApi":{HOST_API_VERSION},
                "spawn":{{"readinessStrategy":{{"kind":"fixed_delay","durationMs":0}},
                         "taskDelivery":"pty_input","verifySubmit":{{"kind":"none"}}}},
                "capabilities":{{"{cap_field}":true}},
                "requiresReload":{requires_reload},
                "entry":"plugin.js"
            }}"#
    )
}

#[test]
fn requires_reload_plugin_installed_live_is_pending_not_active() {
    // §1.8 P1: a plugin declaring requiresReload cannot hot-activate. A fresh
    // install after daemon start is marked pending_reload and its capability
    // is NOT registered live — the banner/CLI tell the user to reload.
    let root = tempfile::tempdir().unwrap();
    let registry = Arc::new(AgentRegistry::new());
    let caps = test_caps();
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registry.clone(),
        caps.clone(),
    );
    // No seed() — simulates a fresh install after startup (id not activated
    // at startup).
    let dir = root.path().join("needsreload");
    write(
        &dir.join("plugin.json"),
        &capability_manifest_full("needsreload", "statusBubble", true),
    );
    write(&dir.join("plugin.js"), "module.exports.default = {};");

    watcher.tick();

    assert!(
        !caps.bubble.contains("needsreload"),
        "requiresReload capability must NOT hot-activate"
    );
    assert!(
        watcher
            .pending_reload_ids()
            .contains(&"needsreload".to_string()),
        "fresh requiresReload install must be pending_reload"
    );

    // Uninstall clears the pending flag.
    std::fs::remove_dir_all(&dir).unwrap();
    watcher.tick();
    assert!(!watcher
        .pending_reload_ids()
        .contains(&"needsreload".to_string()));
}

#[test]
fn seeded_requires_reload_plugin_is_not_pending() {
    // A plugin present at startup (seeded as activated) is already active from
    // the full process start — a later reload must NOT mark it pending.
    let root = tempfile::tempdir().unwrap();
    let registry = Arc::new(AgentRegistry::new());
    let caps = test_caps();
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registry.clone(),
        caps.clone(),
    );
    let dir = root.path().join("startupreload");
    write(
        &dir.join("plugin.json"),
        &capability_manifest_full("startupreload", "statusBubble", true),
    );
    write(&dir.join("plugin.js"), "module.exports.default = {};");
    // Seed: mark as activated-at-startup (lib.rs already registered it).
    watcher.seed();
    assert!(!watcher
        .pending_reload_ids()
        .contains(&"startupreload".to_string()));

    // Edit triggers a reload — still not pending (it was active at startup).
    std::thread::sleep(Duration::from_millis(1100));
    write(&dir.join("plugin.js"), "module.exports.default = { v: 2 };");
    watcher.tick();
    assert!(!watcher
        .pending_reload_ids()
        .contains(&"startupreload".to_string()));
}

fn plugin_for(id: &str) -> String {
    format!(
        r#"module.exports.default = {{
                detectFromTitle: function (t) {{ return t.indexOf("{id}") !== -1; }}
            }};"#
    )
}

#[test]
fn tick_registers_new_plugins_added_after_start() {
    let root = tempfile::tempdir().unwrap();
    let registry = Arc::new(AgentRegistry::new());
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registry.clone(),
        test_caps(),
    );
    // First tick: empty root, nothing to register.
    watcher.tick();
    assert!(watcher.tracked_ids().is_empty());
    assert_eq!(registry.provider_count(), 0);

    // Drop a plugin in.
    let dir = root.path().join("foo");
    write(&dir.join("plugin.json"), &manifest_for("foo"));
    write(&dir.join("plugin.js"), &plugin_for("foo"));

    watcher.tick();
    assert_eq!(watcher.tracked_ids(), vec!["foo".to_string()]);
    assert_eq!(registry.provider_count(), 1);
    assert!(registry
        .with_provider("foo", |p| p.detect_from_title("foo cli"))
        .unwrap_or(false));
}

#[test]
fn tick_hot_reloads_plugin_js_changes() {
    let root = tempfile::tempdir().unwrap();
    let registry = Arc::new(AgentRegistry::new());
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registry.clone(),
        test_caps(),
    );
    let dir = root.path().join("bar");
    write(&dir.join("plugin.json"), &manifest_for("bar"));
    write(&dir.join("plugin.js"), &plugin_for("v1"));
    watcher.tick();
    assert!(registry
        .with_provider("bar", |p| p.detect_from_title("v1"))
        .unwrap_or(false));
    assert!(!registry
        .with_provider("bar", |p| p.detect_from_title("v2"))
        .unwrap_or(true));

    // Sleep a tick so mtime resolution can register the change.
    std::thread::sleep(Duration::from_millis(1100));
    write(&dir.join("plugin.js"), &plugin_for("v2"));
    watcher.tick();

    // Same provider Arc<PluginVm> now serves new JS.
    assert!(registry
        .with_provider("bar", |p| p.detect_from_title("v2"))
        .unwrap_or(false));
    assert!(!registry
        .with_provider("bar", |p| p.detect_from_title("v1"))
        .unwrap_or(true));
}

#[test]
fn duplicate_roots_only_load_the_winning_plugin() {
    let workspace = tempfile::tempdir().unwrap();
    let user = tempfile::tempdir().unwrap();
    let registry = Arc::new(AgentRegistry::new());
    let watcher = HotReloadRegistry::new(
        vec![workspace.path().to_path_buf(), user.path().to_path_buf()],
        registry.clone(),
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
    assert!(registry
        .with_provider("dupe", |p| p.detect_from_title("user cli"))
        .unwrap_or(false));
    assert!(!registry
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
    let registry = Arc::new(AgentRegistry::new());
    let watcher = HotReloadRegistry::new(
        vec![workspace.path().to_path_buf(), user.path().to_path_buf()],
        registry.clone(),
        test_caps(),
    );

    let workspace_dir = workspace.path().join("provider-dupe");
    write(&workspace_dir.join("plugin.json"), &manifest_for("dupe"));
    write(&workspace_dir.join("plugin.js"), &plugin_for("workspace"));
    let user_dir = user.path().join("dupe");
    write(&user_dir.join("plugin.json"), &manifest_for("dupe"));
    write(&user_dir.join("plugin.js"), &plugin_for("user"));

    watcher.tick();
    assert!(registry
        .with_provider("dupe", |p| p.detect_from_title("user cli"))
        .unwrap_or(false));

    std::fs::remove_dir_all(&user_dir).unwrap();
    watcher.tick();

    assert!(registry
        .with_provider("dupe", |p| p.detect_from_title("workspace cli"))
        .unwrap_or(false));
    assert!(!registry
        .with_provider("dupe", |p| p.detect_from_title("user cli"))
        .unwrap_or(true));
}

#[test]
fn tick_registers_capability_plugin_live_without_restart() {
    // §1.8 P1: installing a capability-only plugin must re-register its
    // capability registry live, so e.g. a status bubble appears without a
    // daemon restart (the git-bubble-dark-until-restart symptom).
    let root = tempfile::tempdir().unwrap();
    let registry = Arc::new(AgentRegistry::new());
    let caps = test_caps();
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registry.clone(),
        caps.clone(),
    );
    let dir = root.path().join("bubbler");
    write(
        &dir.join("plugin.json"),
        &capability_manifest_for("bubbler", "statusBubble"),
    );
    write(&dir.join("plugin.js"), "module.exports.default = {};");

    watcher.tick();

    // Capability-only plugin: NOT an agent provider, but its bubble
    // capability is live in the registry.
    assert_eq!(registry.provider_count(), 0);
    assert!(
        caps.bubble.contains("bubbler"),
        "status bubble must register live on install"
    );
}

#[test]
fn tick_deregisters_capability_on_folder_removal() {
    // §1.8 P1: uninstall must deregister the capability — no ghost bubble.
    let root = tempfile::tempdir().unwrap();
    let registry = Arc::new(AgentRegistry::new());
    let caps = test_caps();
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registry.clone(),
        caps.clone(),
    );
    let dir = root.path().join("bubbler");
    write(
        &dir.join("plugin.json"),
        &capability_manifest_for("bubbler", "statusBubble"),
    );
    write(&dir.join("plugin.js"), "module.exports.default = {};");
    watcher.tick();
    assert!(caps.bubble.contains("bubbler"));

    std::fs::remove_dir_all(&dir).unwrap();
    watcher.tick();
    assert!(
        !caps.bubble.contains("bubbler"),
        "capability must be deregistered on uninstall"
    );
}

#[test]
fn config_yaml_edit_restamps_settings_without_vm_rebuild() {
    // HIGH-2 (CRITICAL): editing the per-plugin config.yaml must NOT rebuild
    // the VM — a rebuild wipes the plugin's module-scope in-memory sessions,
    // silently bricking every open chatBackend shell. Instead the watcher
    // re-stamps host.settingsJson() into the EXISTING VM. Assert: a
    // config-only change does NOT increment load_count (no rebuild), AND the
    // live VM observes the new settings on its next read.
    let root = tempfile::tempdir().unwrap();
    let registry = Arc::new(AgentRegistry::new());
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registry.clone(),
        test_caps(),
    );
    let dir = root.path().join("cfgplugin");
    write(&dir.join("plugin.json"), &manifest_for("cfgplugin"));
    write(
        &dir.join("plugin.js"),
        r#"module.exports.default = {
                detectFromTitle: function (t) { return false; },
                settings: function () { return JSON.parse(host.settingsJson()); }
            };"#,
    );
    write(&dir.join("config.yaml"), "engine: whisper\n");
    watcher.tick();
    assert_eq!(watcher.load_count(), 1);

    // The watcher tracks the live VM; it reads the initial settings.
    let vm = watcher
        .existing_vm("cfgplugin")
        .expect("VM tracked after load");
    let before: serde_json::Value = super::super::call_json(&vm, "settings", &[]).unwrap();
    assert_eq!(before["engine"], "whisper");

    // No change → no reload, no re-stamp.
    watcher.tick();
    assert_eq!(watcher.load_count(), 1);

    // Edit config.yaml only → re-stamp into the live VM, NOT a rebuild.
    std::thread::sleep(Duration::from_millis(1100));
    write(&dir.join("config.yaml"), "engine: deepgram\n");
    watcher.tick();
    assert_eq!(
        watcher.load_count(),
        1,
        "a config-only edit must NOT rebuild the VM (a rebuild bricks live sessions)"
    );

    // Same VM Arc, new settings visible — proves the re-stamp reached the
    // existing VM rather than a fresh one.
    let after: serde_json::Value = super::super::call_json(&vm, "settings", &[]).unwrap();
    assert_eq!(
        after["engine"], "deepgram",
        "re-stamped settings must be live on the existing VM"
    );

    // A second config-only edit still no-ops the rebuild path.
    std::thread::sleep(Duration::from_millis(1100));
    write(&dir.join("config.yaml"), "engine: vosk\n");
    watcher.tick();
    assert_eq!(watcher.load_count(), 1);
    let again: serde_json::Value = super::super::call_json(&vm, "settings", &[]).unwrap();
    assert_eq!(again["engine"], "vosk");
}

#[test]
fn tick_unregisters_plugins_after_folder_removal() {
    let root = tempfile::tempdir().unwrap();
    let registry = Arc::new(AgentRegistry::new());
    let watcher = HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registry.clone(),
        test_caps(),
    );
    let dir = root.path().join("baz");
    write(&dir.join("plugin.json"), &manifest_for("baz"));
    write(&dir.join("plugin.js"), &plugin_for("baz"));
    watcher.tick();
    assert_eq!(registry.provider_count(), 1);

    std::fs::remove_dir_all(&dir).unwrap();
    watcher.tick();
    assert!(watcher.tracked_ids().is_empty());
    assert_eq!(registry.provider_count(), 0);
}

#[test]
fn concurrent_tick_and_reload_does_not_deadlock() {
    // Lock-fix regression: tick()'s removal path used to hold the `state`
    // mutex across vms/pending_reload/activated_startup ops AND the
    // agent/capability registry calls — inverting the load order and risking a
    // non-reentrant-mutex deadlock. The fix collects removed ids under a
    // short-held `state` lock, drops it, THEN unregisters. Hammer tick() from
    // several threads while a churner installs/removes a plugin: if `state` is
    // still held across the registry ops, the interleaving wedges and this
    // test hangs (caught by the harness timeout). Reaching the asserts proves
    // the guard is released before the registry work.
    use std::sync::atomic::{AtomicBool, Ordering};
    let root = tempfile::tempdir().unwrap();
    let registry = Arc::new(AgentRegistry::new());
    let watcher = Arc::new(HotReloadRegistry::new(
        vec![root.path().to_path_buf()],
        registry,
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
                // Drain a final tick after the churner stops.
                watcher.tick();
            })
        })
        .collect();

    churner.join().unwrap();
    for t in tickers {
        t.join().unwrap();
    }

    // Quiesce: with the churn dir gone, a final tick must leave nothing tracked
    // and no provider registered — proving the removal path ran to completion
    // (not wedged) and stayed consistent under concurrency.
    std::fs::remove_dir_all(&dir).ok();
    watcher.tick();
    assert!(watcher.tracked_ids().is_empty());
}
