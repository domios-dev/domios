use super::*;
use codeterm_plugin::js_runtime::{CapabilityRegistries, PluginManifest, PluginVm};

fn registries(registry: Arc<SecretRegistry>) -> CapabilityRegistries {
    CapabilityRegistries {
        decision_model: Arc::new(codeterm_plugin::capability::CapabilityRegistry::new()),
        transcribe: Arc::new(codeterm_plugin::transcribe::TranscribeRegistry::new()),
        converter: Arc::new(codeterm_plugin::converter::ConverterRegistry::new()),
        graph_nodes: Arc::new(codeterm_plugin::graph_nodes::GraphNodeRegistry::default()),
        event_source: Arc::new(codeterm_plugin::event_source::EventSourceRegistry::default()),
        glance: Arc::new(codeterm_plugin::glance::GlanceRegistry::new()),
        bubble: Arc::new(codeterm_plugin::bubble::BubbleRegistry::new()),
        view: Arc::new(codeterm_plugin::view_host::ViewRegistry::new()),
        chatbackend: Arc::new(codeterm_plugin::chatbackend_provider::ChatBackendRegistry::new()),
        chat_preprocessor: Arc::new(
            codeterm_plugin::chatpreprocessor::ChatPreprocessorRegistry::new(),
        ),
        agent_command: Arc::new(codeterm_plugin::agent_commands::AgentCommandRegistry::new()),
        webhook_receiver: Arc::new(codeterm_plugin::webhook::WebhookRegistry::new()),
        local_recording: Arc::new(
            codeterm_plugin::local_recording::LocalRecordingRegistry::default(),
        ),
        tab_menu: Arc::new(codeterm_plugin::tab_menu::TabMenuRegistry::new()),
        surfaces: Arc::new(codeterm_plugin::surfaces::SurfaceRegistry::new()),
        secret: Some(Arc::new(crate::js_backend::AppSecretBackendRegistrar::new(
            registry,
        ))),
    }
}

fn plugin_vm(value: &str) -> Arc<PluginVm> {
    Arc::new(
        PluginVm::from_source(
            "reload-vault",
            format!(
                r#"module.exports.default = {{
                    secretGetItem(name) {{
                        return {{ok: {{item_id: 'id', name, value: '{value}'}}}};
                    }},
                    secretStatus() {{return {{status: 'unlocked', user: '{value}', transient: true}};}}
                }};"#
            ),
        )
        .unwrap(),
    )
}

#[tokio::test]
async fn plugin_reload_uses_new_vm_without_reselecting_active_store() {
    let registry = Arc::new(SecretRegistry::new());
    let caps = registries(registry.clone());
    let mut manifest: PluginManifest = serde_json::from_value(serde_json::json!({
        "id": "reload-vault", "version": "1", "displayName": "Reload vault",
        "entry": "plugin.js", "capabilities": {"secretBackend": true}
    }))
    .unwrap();
    let first = plugin_vm("first");
    caps.register_plugin(&manifest, &first);
    let state = crate::new_state(
        &codeterm_config::schema::SecretsConfig {
            backend: manifest.id.clone(),
            ..Default::default()
        },
        &registry,
    );
    let active = state.read().await.clone();
    assert_eq!(
        active.get("token").await.unwrap().value.expose_secret(),
        "first"
    );
    assert!(!active.capabilities().auto_unlock);

    manifest.capabilities.secret_backend =
        serde_json::from_value(serde_json::json!({"autoUnlock": true})).unwrap();
    let second = plugin_vm("second");
    caps.register_plugin(&manifest, &second);
    assert!(Arc::ptr_eq(&active, &*state.read().await));
    assert_eq!(
        active.get("token").await.unwrap().value.expose_secret(),
        "second"
    );
    assert!(active.capabilities().auto_unlock);
    assert!(
        matches!(active.status().await, StoreStatus::Unlocked { user: Some(user), .. } if user == "second")
    );

    caps.deregister_plugin(&manifest.id);
    assert!(matches!(
        active.get("token").await,
        Err(SecretError::Backend { .. })
    ));
    assert!(matches!(
        active.status().await,
        StoreStatus::Unavailable { .. }
    ));
    assert!(!active.capabilities().auto_unlock);
    assert!(!active.allows_account_credentials());
    assert!(!active.auto_unlock_armed());

    caps.register_plugin(&manifest, &second);
    assert_eq!(
        active.get("token").await.unwrap().value.expose_secret(),
        "second"
    );
}

#[tokio::test]
async fn missing_configured_backend_falls_back_to_local_and_activates_live_on_registration() {
    let registry = Arc::new(SecretRegistry::new());
    let registered_events = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
    {
        let events = registered_events.clone();
        registry.on_register(Arc::new(move |id| events.lock().push(id.to_string())));
    }
    let caps = registries(registry.clone());
    let state = crate::new_state(
        &codeterm_config::schema::SecretsConfig {
            backend: "reload-vault".into(),
            ..Default::default()
        },
        &registry,
    );
    let active = state.read().await.clone();
    assert_eq!(active.id(), "local");
    assert_eq!(
        active.fallback(),
        Some(crate::BackendFallback {
            configured_backend: "reload-vault".into(),
            active_backend: "local".into(),
        })
    );

    let manifest: PluginManifest = serde_json::from_value(serde_json::json!({
        "id": "reload-vault", "version": "1.0.12", "displayName": "Reload vault",
        "entry": "plugin.js", "capabilities": {"secretBackend": true}
    }))
    .unwrap();
    let first = plugin_vm("live-v1");
    caps.register_plugin(&manifest, &first);

    assert_eq!(*registered_events.lock(), vec!["reload-vault".to_string()]);
    assert!(Arc::ptr_eq(&active, &*state.read().await));
    assert_eq!(active.id(), "reload-vault");
    assert_eq!(active.fallback(), None);
    assert_eq!(
        active.get("token").await.unwrap().value.expose_secret(),
        "live-v1"
    );
    assert!(matches!(
        active.status().await,
        StoreStatus::Unlocked { user: Some(user), .. } if user == "live-v1"
    ));

    caps.deregister_plugin(&manifest.id);
    assert_eq!(active.id(), "reload-vault");
    assert_eq!(active.fallback(), None);
    assert!(matches!(
        active.status().await,
        StoreStatus::Unavailable { .. }
    ));
}
