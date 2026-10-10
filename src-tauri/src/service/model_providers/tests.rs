use super::*;
use crate::secrets::{mock::MockStore, SecretStore};
use codeterm_plugin::model_providers::{ConfigUpdate, EndpointKind};
use std::path::PathBuf;

struct Adapter {
    path: PathBuf,
    inner: Arc<dyn ModelProviderConfig>,
}

impl ModelProviderConfig for Adapter {
    fn read_presets(
        &self,
        config: &Value,
    ) -> Result<Vec<codeterm_plugin::model_providers::ModelPreset>, String> {
        self.inner.read_presets(config)
    }
    fn apply_presets(
        &self,
        config: &Value,
        presets: &[codeterm_plugin::model_providers::ModelPreset],
    ) -> Result<ConfigUpdate, String> {
        self.inner.apply_presets(config, presets)
    }
    fn preset_launch_args(&self, name: &str) -> Result<Vec<String>, String> {
        self.inner.preset_launch_args(name)
    }
    fn path(&self) -> Result<PathBuf, String> {
        Ok(self.path.clone())
    }
    fn read(&self, config: &Value) -> Result<Vec<ModelProviderDefinition>, String> {
        self.inner.read(config)
    }
    fn apply(
        &self,
        config: &Value,
        providers: &[ModelProviderDefinition],
    ) -> Result<ConfigUpdate, String> {
        self.inner.apply(config, providers)
    }
    fn key_env_name(&self, id: &str) -> Result<String, String> {
        self.inner.key_env_name(id)
    }
}

#[test]
fn model_provider_preset_owned_roundtrip_foreign_entries_and_launch_resolution() {
    use codeterm_plugin::model_providers::{ModelPreset, PresetParams};
    let dir = tempfile::tempdir().unwrap();
    let adapter = adapter(dir.path().join("config.json"));
    let store = store();
    let foreign = json!({"agent":{"foreign":{"model":"foreign/model","prompt":"opaque"}},"provider":{"foreign":{"npm":"foreign"}}});
    std::fs::write(&adapter.path, serde_json::to_vec(&foreign).unwrap()).unwrap();
    let preset = ModelPreset {
        name: "demo".into(),
        model: "endpoint/model".into(),
        prompt: "opaque-prompt".into(),
        params: PresetParams {
            temperature: Some(0.2),
            top_p: Some(0.8),
            steps: Some(12),
        },
    };
    execute(
        &store,
        "fixture",
        &adapter,
        ModelProviderRequest::PresetAdd {
            preset: preset.clone(),
        },
    )
    .unwrap();
    let listed = execute(
        &store,
        "fixture",
        &adapter,
        ModelProviderRequest::PresetList,
    )
    .unwrap();
    assert_eq!(listed["presets"], json!([preset]));
    assert_eq!(
        read_config(&adapter.path).unwrap()["provider"],
        foreign["provider"]
    );
    assert_eq!(
        presets::resolve(&adapter, "demo", None, &[]).unwrap(),
        (
            "endpoint/model".into(),
            vec!["--agent".into(), "demo".into()]
        )
    );
    assert_eq!(
        presets::resolve(&adapter, "demo", Some("other/model"), &[]).unwrap_err(),
        ModelProviderError::Conflict
    );
    assert_eq!(
        presets::resolve(&adapter, "demo", None, &["--agent=foreign".into()]).unwrap_err(),
        ModelProviderError::Conflict
    );
    let bytes = std::fs::read(&adapter.path).unwrap();
    let mut collision = preset.clone();
    collision.name = "foreign".into();
    assert_eq!(
        execute(
            &store,
            "fixture",
            &adapter,
            ModelProviderRequest::PresetAdd { preset: collision }
        )
        .unwrap_err(),
        ModelProviderError::Conflict
    );
    assert_eq!(
        execute(
            &store,
            "fixture",
            &adapter,
            ModelProviderRequest::PresetRemove {
                name: "foreign".into()
            }
        )
        .unwrap_err(),
        ModelProviderError::NotFound
    );
    assert_eq!(std::fs::read(&adapter.path).unwrap(), bytes);
    execute(
        &store,
        "fixture",
        &adapter,
        ModelProviderRequest::PresetRemove {
            name: "demo".into(),
        },
    )
    .unwrap();
    assert_eq!(read_config(&adapter.path).unwrap(), foreign);
}

#[test]
fn model_provider_preset_invalid_params_do_not_write_config() {
    use codeterm_plugin::model_providers::{ModelPreset, PresetParams};
    let dir = tempfile::tempdir().unwrap();
    let adapter = adapter(dir.path().join("config.json"));
    for params in [
        PresetParams {
            temperature: Some(f64::NAN),
            ..Default::default()
        },
        PresetParams {
            top_p: Some(2.0),
            ..Default::default()
        },
        PresetParams {
            steps: Some(0),
            ..Default::default()
        },
    ] {
        let preset = ModelPreset {
            name: "demo".into(),
            model: "endpoint/model".into(),
            prompt: "opaque".into(),
            params,
        };
        assert_eq!(
            execute(
                &store(),
                "fixture",
                &adapter,
                ModelProviderRequest::PresetAdd { preset }
            )
            .unwrap_err(),
            ModelProviderError::InvalidRequest
        );
        assert!(!adapter.path.exists());
    }
}

fn adapter(path: PathBuf) -> Adapter {
    let manifest =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../plugins/provider-opencode/plugin.json");
    let provider = codeterm_plugin::js_runtime::load_one(&manifest).unwrap();
    Adapter {
        path,
        inner: provider.model_provider_config().unwrap(),
    }
}

fn store() -> SecretStoreState {
    Arc::new(tokio::sync::RwLock::new(
        Arc::new(MockStore::new()) as Arc<dyn SecretStore>
    ))
}

fn definition() -> ModelProviderDefinition {
    ModelProviderDefinition {
        id: "demo".into(),
        kind: EndpointKind::OpenaiCompatible,
        base_url: "http://localhost:1234/v1".into(),
        models: vec!["model-a".into()],
        key_env: None,
        name: None,
        secret_name: Some("slot".into()),
    }
}

#[test]
fn model_provider_saved_secret_copy_is_owned_and_source_survives_removal() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = adapter(dir.path().join("opencode.json"));
    let store = store();
    tauri::async_runtime::block_on(async {
        store
            .read()
            .await
            .save(SaveRequest {
                name: "source".into(),
                value: SecretString::from("opaque-fixture"),
                notes: Some("foreign-owner".into()),
                folder: None,
                organization: None,
                collection: None,
                overwrite: false,
            })
            .await
            .unwrap();
    });
    execute(
        &store,
        "fixture",
        &adapter,
        ModelProviderRequest::AddFromSecret {
            provider: definition(),
            source_secret: "source".into(),
        },
    )
    .unwrap();
    let providers = adapter.read(&read_config(&adapter.path).unwrap()).unwrap();
    assert_eq!(
        providers[0].secret_name.as_deref(),
        Some("domios/model-provider/demo")
    );
    assert!(get_secret(&store, "fixture", &providers[0]).is_ok());
    execute(
        &store,
        "fixture",
        &adapter,
        ModelProviderRequest::Remove {
            name: "demo".into(),
        },
    )
    .unwrap();
    tauri::async_runtime::block_on(async {
        assert_eq!(
            store
                .read()
                .await
                .get("source")
                .await
                .unwrap()
                .notes
                .as_deref(),
            Some("foreign-owner")
        );
        assert!(store
            .read()
            .await
            .get("domios/model-provider/demo")
            .await
            .is_err());
    });
}

#[test]
fn model_provider_add_launch_remove_preserves_foreign_config_and_secret_confinement() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = adapter(dir.path().join("opencode.json"));
    let store = store();
    let foreign =
        json!({"provider":{"foreign":{"npm":"foreign"}},"agent":{"owner":{"prompt":"opaque"}}});
    std::fs::write(&adapter.path, serde_json::to_vec(&foreign).unwrap()).unwrap();
    let key = ["generated", "fixture", "value"].join("-");
    let added = execute(
        &store,
        "arbitrary-plugin",
        &adapter,
        ModelProviderRequest::Add {
            provider: definition(),
            value: SecretString::from(key.clone()),
        },
    )
    .unwrap();
    let bytes = std::fs::read_to_string(&adapter.path).unwrap();
    assert!(!bytes.contains(&key));
    assert!(!added.to_string().contains(&key));
    assert!(bytes.contains("{env:DOMIOS_PROVIDER_DEMO_KEY}"));
    let providers = adapter.read(&read_config(&adapter.path).unwrap()).unwrap();
    let env = launch_env(&store, "arbitrary-plugin", &adapter, &providers).unwrap();
    assert_eq!(
        env,
        HashMap::from([("DOMIOS_PROVIDER_DEMO_KEY".into(), key)])
    );
    execute(
        &store,
        "arbitrary-plugin",
        &adapter,
        ModelProviderRequest::Remove {
            name: "demo".into(),
        },
    )
    .unwrap();
    assert_eq!(read_config(&adapter.path).unwrap(), foreign);
    assert!(
        tauri::async_runtime::block_on(async { store.read().await.get("slot").await }).is_err()
    );
}

#[test]
fn model_provider_foreign_collision_never_creates_a_secret() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = adapter(dir.path().join("config.json"));
    let store = store();
    let config = json!({"provider":{"demo":{"npm":"foreign"}}});
    std::fs::write(&adapter.path, serde_json::to_vec(&config).unwrap()).unwrap();
    assert_eq!(
        execute(
            &store,
            "plugin",
            &adapter,
            ModelProviderRequest::Add {
                provider: definition(),
                value: SecretString::from("opaque")
            }
        )
        .unwrap_err(),
        ModelProviderError::Conflict
    );
    assert_eq!(read_config(&adapter.path).unwrap(), config);
    assert!(
        tauri::async_runtime::block_on(async { store.read().await.get("slot").await }).is_err()
    );
    assert_eq!(
        execute(
            &store,
            "plugin",
            &adapter,
            ModelProviderRequest::Remove {
                name: "demo".into()
            }
        )
        .unwrap_err(),
        ModelProviderError::NotFound
    );
}

#[test]
fn model_provider_secret_ownership_refuses_foreign_secret_cleanup_and_launch() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = adapter(dir.path().join("config.json"));
    let store = store();
    execute(
        &store,
        "owner",
        &adapter,
        ModelProviderRequest::Add {
            provider: definition(),
            value: SecretString::from("opaque"),
        },
    )
    .unwrap();
    let config = read_config(&adapter.path).unwrap();
    let providers = adapter.read(&config).unwrap();
    assert_eq!(
        launch_env(&store, "foreign-owner", &adapter, &providers).unwrap_err(),
        ModelProviderError::SecretOwnership
    );
    assert_eq!(
        execute(
            &store,
            "foreign-owner",
            &adapter,
            ModelProviderRequest::Remove {
                name: "demo".into()
            }
        )
        .unwrap_err(),
        ModelProviderError::SecretOwnership
    );
    assert_eq!(read_config(&adapter.path).unwrap(), config);
}

#[test]
fn model_provider_names_have_unique_env_encoding() {
    for id in [
        "",
        "MyLLM",
        "my_llm",
        "my--llm",
        "my.llm",
        "../escape",
        "-demo",
    ] {
        assert!(!valid_id(id));
    }
    for id in ["demo", "my-llm", "demo2"] {
        assert!(valid_id(id));
    }
}

#[test]
fn model_provider_config_lock_is_immediate_refusal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    let first = config_lock(&path).unwrap();
    let inherited = first.0.try_clone().unwrap();
    assert_eq!(config_lock(&path).unwrap_err(), ModelProviderError::Busy);
    drop(first);
    assert!(config_lock(&path).is_ok());
    drop(inherited);
}

#[test]
fn model_provider_bad_json_is_never_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = adapter(dir.path().join("config.json"));
    std::fs::write(&adapter.path, b"invalid").unwrap();
    assert_eq!(
        execute(&store(), "owner", &adapter, ModelProviderRequest::List).unwrap_err(),
        ModelProviderError::Config
    );
    assert_eq!(std::fs::read(&adapter.path).unwrap(), b"invalid");
}

#[test]
fn model_provider_routing_uses_the_declared_capability_and_refuses_ambiguity() {
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    let manifest: PluginManifest = serde_json::from_value(json!({
        "id":"arbitrary-owner", "version":"0.1.0", "displayName":"Fixture",
        "capabilities":{"agentProvider":true,"modelProviderConfig":true}
    }))
    .unwrap();
    let provider: Arc<dyn AgentProvider> = Arc::new(JsProvider::new(
        manifest,
        Arc::new(
            PluginVm::from_source("arbitrary-owner", "module.exports.default = {};".into())
                .unwrap(),
        ),
    ));
    let native: Arc<dyn AgentProvider> =
        Arc::new(codeterm_plugin::agent_claude::provider::ClaudeProvider::new());
    assert!(requires_fresh_pty(&*provider));
    assert!(!requires_fresh_pty(&*native));
    assert_eq!(
        select_owner(&[native.clone(), provider.clone()])
            .unwrap()
            .id(),
        "arbitrary-owner"
    );
    assert!(matches!(
        select_owner(&[native]),
        Err(ModelProviderError::Unsupported)
    ));
    assert!(matches!(
        select_owner(&[provider.clone(), provider]),
        Err(ModelProviderError::Ambiguous)
    ));
}

#[test]
fn model_provider_existing_secret_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = adapter(dir.path().join("config.json"));
    let store = store();
    let saved = tauri::async_runtime::block_on(async {
        store
            .read()
            .await
            .save(SaveRequest {
                name: "slot".into(),
                value: SecretString::from("foreign-value"),
                notes: None,
                folder: None,
                organization: None,
                collection: None,
                overwrite: false,
            })
            .await
            .unwrap()
    });
    assert_eq!(
        execute(
            &store,
            "owner",
            &adapter,
            ModelProviderRequest::Add {
                provider: definition(),
                value: SecretString::from("new-value")
            }
        )
        .unwrap_err(),
        ModelProviderError::Conflict
    );
    let secret = tauri::async_runtime::block_on(async {
        store.read().await.get(&saved.item_id).await.unwrap()
    });
    assert_eq!(secret.value.expose_secret(), "foreign-value");
    assert!(!adapter.path.exists());
}

#[test]
fn model_provider_config_failure_cleans_the_new_secret() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct FailSecondApply {
        adapter: Adapter,
        calls: AtomicUsize,
    }
    impl ModelProviderConfig for FailSecondApply {
        fn path(&self) -> Result<PathBuf, String> {
            self.adapter.path()
        }
        fn read(&self, config: &Value) -> Result<Vec<ModelProviderDefinition>, String> {
            self.adapter.read(config)
        }
        fn key_env_name(&self, id: &str) -> Result<String, String> {
            self.adapter.key_env_name(id)
        }
        fn apply(
            &self,
            config: &Value,
            providers: &[ModelProviderDefinition],
        ) -> Result<ConfigUpdate, String> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
                return Err("injected failure".into());
            }
            self.adapter.apply(config, providers)
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let store = store();
    let adapter = FailSecondApply {
        adapter: adapter(dir.path().join("config.json")),
        calls: AtomicUsize::new(0),
    };
    assert_eq!(
        execute(
            &store,
            "owner",
            &adapter,
            ModelProviderRequest::Add {
                provider: definition(),
                value: SecretString::from("opaque")
            }
        )
        .unwrap_err(),
        ModelProviderError::Config
    );
    assert!(
        tauri::async_runtime::block_on(async { store.read().await.get("slot").await }).is_err()
    );
    assert!(!adapter.adapter.path.exists());
}

#[test]
fn model_provider_scoped_owner_requires_the_declared_capability() {
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    let manifest: PluginManifest = serde_json::from_value(json!({
        "id":"scoped-owner", "version":"0.1.0", "displayName":"Fixture",
        "capabilities":{"agentProvider":true,"modelProviderConfig":true}
    }))
    .unwrap();
    let provider: Arc<dyn AgentProvider> = Arc::new(JsProvider::new(
        manifest,
        Arc::new(
            PluginVm::from_source("scoped-owner", "module.exports.default = {};".into()).unwrap(),
        ),
    ));
    let native: Arc<dyn AgentProvider> =
        Arc::new(codeterm_plugin::agent_claude::provider::ClaudeProvider::new());
    let providers = [native.clone(), provider.clone(), provider];
    assert!(matches!(
        select_owner_for(&providers, None),
        Err(ModelProviderError::Ambiguous)
    ));
    assert_eq!(
        select_owner_for(&providers, Some("scoped-owner"))
            .unwrap()
            .id(),
        "scoped-owner"
    );
    assert!(matches!(
        select_owner_for(&providers, Some(native.id())),
        Err(ModelProviderError::Unsupported)
    ));
}

#[test]
fn model_provider_update_rewrites_endpoint_and_rotates_owned_key_outside_config() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = adapter(dir.path().join("opencode.json"));
    let store = store();
    let first = ["first", "fixture", "value"].join("-");
    execute(
        &store,
        "owner",
        &adapter,
        ModelProviderRequest::Add {
            provider: definition(),
            value: SecretString::from(first),
        },
    )
    .unwrap();
    let rotated = ["rotated", "fixture", "value"].join("-");
    let mut patch = definition();
    patch.base_url = "http://localhost:4321/v1".into();
    patch.models = vec!["model-b".into(), "model-c".into()];
    patch.secret_name = None;
    let updated = execute(
        &store,
        "owner",
        &adapter,
        ModelProviderRequest::Update {
            provider: patch,
            value: Some(SecretString::from(rotated.clone())),
        },
    )
    .unwrap();
    assert!(!updated.to_string().contains(&rotated));
    let bytes = std::fs::read_to_string(&adapter.path).unwrap();
    assert!(!bytes.contains(&rotated));
    let providers = adapter.read(&read_config(&adapter.path).unwrap()).unwrap();
    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].base_url, "http://localhost:4321/v1");
    assert_eq!(providers[0].models, vec!["model-b", "model-c"]);
    assert_eq!(providers[0].secret_name.as_deref(), Some("slot"));
    let env = launch_env(&store, "owner", &adapter, &providers).unwrap();
    assert_eq!(
        env,
        HashMap::from([("DOMIOS_PROVIDER_DEMO_KEY".into(), rotated)])
    );
}

#[test]
fn model_provider_update_refuses_unknown_endpoint_and_foreign_owner() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = adapter(dir.path().join("opencode.json"));
    let store = store();
    let update = || ModelProviderRequest::Update {
        provider: definition(),
        value: Some(SecretString::from("opaque-fixture")),
    };
    assert!(matches!(
        execute(&store, "owner", &adapter, update()),
        Err(ModelProviderError::NotFound)
    ));
    execute(
        &store,
        "owner",
        &adapter,
        ModelProviderRequest::Add {
            provider: definition(),
            value: SecretString::from("original-fixture"),
        },
    )
    .unwrap();
    let before = std::fs::read(&adapter.path).unwrap();
    assert!(matches!(
        execute(&store, "intruder", &adapter, update()),
        Err(ModelProviderError::SecretOwnership)
    ));
    assert_eq!(std::fs::read(&adapter.path).unwrap(), before);
}

#[test]
fn model_provider_discover_needs_exactly_one_key_source() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = adapter(dir.path().join("opencode.json"));
    let store = store();
    let discover = |value: Option<&str>, source_secret: Option<&str>| {
        execute(
            &store,
            "owner",
            &adapter,
            ModelProviderRequest::Discover {
                kind: EndpointKind::OpenaiCompatible,
                base_url: "http://localhost:1234/v1".into(),
                value: value.map(SecretString::from),
                source_secret: source_secret.map(str::to_owned),
            },
        )
    };
    assert!(matches!(
        discover(None, None),
        Err(ModelProviderError::InvalidRequest)
    ));
    assert!(matches!(
        discover(Some("opaque"), Some("source")),
        Err(ModelProviderError::InvalidRequest)
    ));
    assert!(matches!(
        discover(Some(""), None),
        Err(ModelProviderError::InvalidRequest)
    ));
    assert!(matches!(
        discover(None, Some("missing")),
        Err(ModelProviderError::NotFound)
    ));
    assert!(!adapter.path.exists());
}

#[test]
fn model_provider_wire_accepts_update_and_discover_without_a_key() {
    let update: ModelProviderRequest = serde_json::from_value(json!({
        "action":"update",
        "provider":{"id":"demo","kind":"anthropic","baseUrl":"https://example.test","models":[]}
    }))
    .unwrap();
    assert!(matches!(
        update,
        ModelProviderRequest::Update { value: None, .. }
    ));
    let discover: ModelProviderRequest = serde_json::from_value(json!({
        "action":"discover","kind":"openai-compatible","base_url":"https://example.test",
        "source_secret":"saved"
    }))
    .unwrap();
    assert!(matches!(
        discover,
        ModelProviderRequest::Discover {
            value: None,
            source_secret: Some(_),
            ..
        }
    ));
}
