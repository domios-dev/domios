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
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn model_provider_decision_exec_uses_owned_launch_env_without_exposing_keys() {
    use codeterm_plugin::{
        js_runtime::{JsDecisionModel, JsProvider, PluginManifest, PluginVm},
        PluginHostCtx,
    };
    use codeterm_types::decision::{
        DecisionAnswer, DecisionModel, DecisionQuestion, DecisionRequest, TokenMode,
    };
    let mut fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    fixture.service.secrets = store();
    let path = fixture.temp.path().join("decision-config.json");
    let manifest: PluginManifest = serde_json::from_value(json!({"id":"arbitrary-owner","version":"0.1.0","displayName":"Fixture","capabilities":{"agentProvider":true,"modelProviderConfig":true,"decisionModel":true}})).unwrap();
    let bundle = include_str!("../../../../plugins/provider-opencode/plugin.js");
    let source = format!(
        r#"{bundle}
        module.exports.default.modelProviderConfigPath = () => {};
        module.exports.default.decide = () => host.exec.async({{
            bin:'node', args:['-e',"process.stdout.write(JSON.stringify({{type:'noul',p:process.env.DOMIOS_PROVIDER_DEMO_KEY ? 0.8 : 0}}))"], timeoutMs:2000
        }}, result => JSON.parse(result.stdout));
    "#,
        serde_json::to_string(&path).unwrap()
    );
    let vm = Arc::new(PluginVm::from_source("arbitrary-owner", source).unwrap());
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(manifest, vm.clone())));
    let host: Arc<dyn PluginHostCtx> = Arc::new(fixture.service.clone());
    vm.set_service(Arc::downgrade(&host));
    let key = ["generated", "decision", "fixture"].join("-");
    fixture
        .service
        .model_provider_command(ModelProviderRequest::Add {
            provider: definition(),
            value: SecretString::from(key.clone()),
        })
        .unwrap();
    let env = host.model_provider_process_env("arbitrary-owner").unwrap();
    assert_eq!(
        env,
        fixture
            .service
            .model_provider_launch_env("arbitrary-owner")
            .unwrap()
    );
    assert_eq!(env.get("DOMIOS_PROVIDER_DEMO_KEY"), Some(&key));
    assert!(host
        .model_provider_process_env("absent-owner")
        .unwrap()
        .is_empty());
    assert!(!std::fs::read_to_string(&path).unwrap().contains(&key));
    let model = JsDecisionModel::new(vm, "arbitrary-owner", TokenMode::None);
    let request = DecisionRequest {
        state: json!({}),
        instructions: "opaque".into(),
        question: DecisionQuestion::Noul { criteria: None },
    };
    assert_eq!(
        model
            .decide(&request, std::time::Instant::now() + Duration::from_secs(3))
            .unwrap(),
        DecisionAnswer::Noul { p: 0.8 }
    );
    fixture
        .service
        .model_provider_command(ModelProviderRequest::Remove {
            name: "demo".into(),
        })
        .unwrap();
    assert!(host
        .model_provider_process_env("arbitrary-owner")
        .unwrap()
        .is_empty());
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
    assert_eq!(config_lock(&path).unwrap_err(), ModelProviderError::Busy);
    drop(first);
    assert!(config_lock(&path).is_ok());
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
