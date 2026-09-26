//! Parity tests: load each bundled JS plugin and assert its output
//! matches the native Rust provider on identical inputs for the
//! pure-logic surface (detection, slash commands, schemas, simple
//! launch/resume builds, JSONL parsing where applicable).
//!
//! These tests document exactly *which* AgentProvider methods are
//! safe to migrate to JS today. Methods that diverge (e.g. Claude's
//! stdin-pipe headless launch, which uses platform-specific shell
//! quoting + temp-file gymnastics that legitimately live in Rust)
//! are intentionally not covered here — the native provider remains
//! authoritative for them.

use std::path::PathBuf;

use crate::{
    chatbackend_provider::{ChatBackendError, ChatBackendProvider, ChatBackendRegistry},
    provider::{AgentLaunchParams, AgentProvider, ModelInfo, ReasoningEffortInfo},
};

use super::{load_one, JsProvider};

#[path = "parity_test/model_catalogue.rs"]
mod model_catalogue;

type PluginsDir = PathBuf;

fn workspace_plugins_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is <repo>/src-tauri/crates/codeterm-plugin; the plugins/
    // dir lives at the repo root — three levels up (crate -> crates -> src-tauri).
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../..")).join("plugins")
}

fn require_plugins() -> Option<PluginsDir> {
    let path = workspace_plugins_root();
    if path.exists() {
        return Some(path);
    }

    if std::env::var("CI").is_ok() {
        panic!("parity tests require bundled JS plugins in CI — build them in the workflow or fix the path");
    }

    eprintln!(
        "SKIPPED: js_runtime parity — plugins dir not found at {}",
        path.display()
    );
    None
}

fn load_js(id: &str) -> JsProvider {
    let manifest = workspace_plugins_root()
        .join(format!("provider-{id}"))
        .join("plugin.json");
    load_one(&manifest).unwrap_or_else(|e| panic!("load {id} plugin: {e}"))
}

fn load_inline_js(id: &str, plugin_js: &str) -> JsProvider {
    use std::sync::Arc;

    let manifest_json = format!(
        r#"{{"id":"{id}","version":"0.1.0","displayName":"Fixture",
                "spawn":{{"readinessStrategy":{{"kind":"fixed_delay","durationMs":0}},
                         "taskDelivery":"pty_input","verifySubmit":{{"kind":"none"}}}}}}"#
    );
    let manifest: super::PluginManifest = serde_json::from_str(&manifest_json).unwrap();
    let vm = super::PluginVm::from_source_with(id, plugin_js.to_string(), Some(manifest_json))
        .expect("fixture vm");
    JsProvider::new(manifest, Arc::new(vm))
}

#[test]
fn claude_update_command_native_js_parity() {
    let Some(_) = require_plugins() else {
        return;
    };
    let native = crate::agent_claude::provider::ClaudeProvider::new();
    let js = load_js("claude");
    for platform in ["macos", "linux", "windows", "unknown"] {
        assert_eq!(js.update_command(platform), native.update_command(platform));
    }
}

struct EnvGuard {
    _inner: codeterm_util::test_env::EnvGuard,
}

impl EnvGuard {
    fn new(vars: &[(&str, &str)]) -> Self {
        let mut inner = codeterm_util::test_env::EnvGuard::lock();
        for (k, v) in vars {
            inner.set(k, v);
        }
        Self { _inner: inner }
    }
}

fn discover_models_fixture() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "live-a".into(),
            display_name: "Live A".into(),
            description: None,
            available: None,
            availability: None,
            featured: None,
            badge: None,
            group: Some("alpha".into()),
            reasoning_efforts: Some(vec![
                ReasoningEffortInfo {
                    id: "max".into(),
                    display_name: "Max".into(),
                    description: Some("Maximum reasoning".into()),
                },
                ReasoningEffortInfo {
                    id: "ultra".into(),
                    display_name: "Ultra".into(),
                    description: None,
                },
            ]),
        },
        ModelInfo {
            id: "live-b".into(),
            display_name: "Live B".into(),
            description: Some("second".into()),
            available: None,
            availability: None,
            featured: None,
            badge: None,
            group: None,
            reasoning_efforts: None,
        },
        // A probe that could not run is a capability answer of its own, so the
        // native and JS paths must agree it is unknown, not unavailable.
        ModelInfo {
            id: "live-c".into(),
            display_name: "Live C".into(),
            description: None,
            available: None,
            availability: Some(crate::provider::ModelAvailability::Unknown),
            featured: None,
            badge: None,
            group: None,
            reasoning_efforts: None,
        },
    ]
}

struct DiscoverModelsNativeStub;

#[async_trait::async_trait]
impl AgentProvider for DiscoverModelsNativeStub {
    fn id(&self) -> &str {
        "discover-fixture"
    }

    fn display_name(&self) -> &str {
        "Discover Fixture"
    }

    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }

    fn readiness_strategy(&self) -> crate::provider::ReadinessStrategy {
        crate::provider::ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::ZERO,
        }
    }

    fn task_delivery(&self) -> crate::provider::TaskDelivery {
        crate::provider::TaskDelivery::PtyInput
    }

    fn provider_readiness_strategy(&self) -> crate::provider::ProviderReadinessStrategy {
        crate::provider::ProviderReadinessStrategy::StructuralComposer {
            timeout: std::time::Duration::from_secs(17),
        }
    }

    fn detect_from_title(&self, _: &str) -> bool {
        false
    }

    fn detect_from_output(&self, _: &str) -> bool {
        false
    }

    fn detect_session_reasoning_effort(&self, _cwd: &str, session_id: &str) -> Option<String> {
        (session_id == "owned-session").then(|| "high".to_string())
    }

    fn session_file_path(&self, _: &str, _: &str) -> Option<std::path::PathBuf> {
        None
    }

    fn detect_session_id(
        &self,
        _: &str,
        _: &[String],
        _: Option<std::time::Duration>,
    ) -> Option<String> {
        None
    }

    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }

    async fn discover_models(&self) -> Option<Vec<ModelInfo>> {
        Some(discover_models_fixture())
    }

    fn model_catalogue_capability(&self) -> crate::provider::ModelCatalogueCapability {
        crate::provider::ModelCatalogueCapability::authoritative_live()
    }
}

struct SessionConfigNativeStub;

impl ChatBackendProvider for SessionConfigNativeStub {
    fn id(&self) -> &str {
        "session-config-fixture"
    }

    fn supports_session_config(&self) -> bool {
        true
    }

    fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
        unreachable!("descriptor parity does not open a session")
    }

    fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
        unreachable!("descriptor parity does not send messages")
    }

    fn poll(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<codeterm_types::agent::chat_types::StructuredChat, ChatBackendError> {
        unreachable!("descriptor parity does not poll sessions")
    }

    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        unreachable!("descriptor parity does not close sessions")
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
        unreachable!("descriptor parity does not list models")
    }
}

#[test]
fn chatbackend_session_config_descriptor_native_js_parity() {
    use std::sync::Arc;

    let manifest_json = r#"{
        "id":"session-config-fixture",
        "version":"0.1.0",
        "displayName":"Session Config Fixture",
        "capabilities":{"chatBackend":{"sessionConfig":true}}
    }"#;
    let manifest: super::PluginManifest = serde_json::from_str(manifest_json).unwrap();
    let vm = super::PluginVm::from_source_with(
        "session-config-fixture",
        "module.exports.default = {};".into(),
        Some(manifest_json.into()),
    )
    .unwrap();
    let js = super::JsChatBackend::from_manifest(Arc::new(vm), &manifest);
    let native = SessionConfigNativeStub;

    assert_eq!(
        js.describe().system_prompt_delivery,
        native.describe().system_prompt_delivery
    );
}

struct ModelCapabilityNativeStub {
    outcome: Result<Vec<ModelInfo>, String>,
}

impl ChatBackendProvider for ModelCapabilityNativeStub {
    fn id(&self) -> &str {
        "chatbackend-model-parity-fixture"
    }

    fn display_name(&self) -> &str {
        "Model parity fixture"
    }

    fn spawnable(&self) -> bool {
        true
    }

    fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
        unreachable!("model parity does not open a session")
    }

    fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
        unreachable!("model parity does not send messages")
    }

    fn poll(
        &self,
        _: &str,
        _: Option<&str>,
    ) -> Result<codeterm_types::agent::chat_types::StructuredChat, ChatBackendError> {
        unreachable!("model parity does not poll sessions")
    }

    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        unreachable!("model parity does not close sessions")
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
        self.outcome.clone().map_err(ChatBackendError::Call)
    }
}

fn model_capability_fixture() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "sentinel-a".into(),
            display_name: "Sentinel A".into(),
            description: Some("ready".into()),
            available: Some(true),
            availability: None,
            featured: None,
            badge: None,
            group: None,
            reasoning_efforts: None,
        },
        ModelInfo {
            id: "sentinel-b".into(),
            display_name: "Sentinel B".into(),
            description: Some("offline".into()),
            available: Some(false),
            availability: None,
            featured: None,
            badge: None,
            group: None,
            reasoning_efforts: None,
        },
    ]
}

fn js_model_capability(source: &str) -> super::JsChatBackend {
    use std::sync::Arc;

    let manifest_json = r#"{
        "id":"chatbackend-model-parity-fixture",
        "version":"0.1.0",
        "displayName":"Model parity fixture",
        "capabilities":{"chatBackend":{"spawnable":true}}
    }"#;
    let manifest: super::PluginManifest = serde_json::from_str(manifest_json).unwrap();
    let vm = super::PluginVm::from_source_with(
        "chatbackend-model-parity-fixture",
        source.into(),
        Some(manifest_json.into()),
    )
    .unwrap();
    super::JsChatBackend::from_manifest(Arc::new(vm), &manifest)
}

#[test]
fn native_and_js_chatbackend_model_capability_are_identical() {
    use std::sync::Arc;

    let native_registry = ChatBackendRegistry::new();
    native_registry.register_and_wait(
        "chatbackend-model-parity-fixture",
        Arc::new(ModelCapabilityNativeStub {
            outcome: Ok(model_capability_fixture()),
        }),
    );
    let native = native_registry
        .descriptor("chatbackend-model-parity-fixture")
        .unwrap();

    let js_registry = ChatBackendRegistry::new();
    js_registry.register_and_wait(
        "chatbackend-model-parity-fixture",
        Arc::new(js_model_capability(
            r#"module.exports.default = {
                listModels() { return [
                    { id: "sentinel-a", displayName: "Sentinel A", description: "ready", available: true },
                    { id: "sentinel-b", displayName: "Sentinel B", description: "offline", available: false }
                ]; }
            };"#,
        )),
    );
    let js = js_registry
        .descriptor("chatbackend-model-parity-fixture")
        .unwrap();

    assert_eq!(native.available_models, js.available_models);
    assert_eq!(native.available_models, model_capability_fixture());
    assert_eq!(native.default_model, None);
    assert_eq!(js.default_model, None);

    let native_missing = ChatBackendRegistry::new();
    native_missing.register_and_wait(
        "chatbackend-model-parity-fixture",
        Arc::new(ModelCapabilityNativeStub {
            outcome: Ok(Vec::new()),
        }),
    );
    let js_missing = ChatBackendRegistry::new();
    js_missing.register_and_wait(
        "chatbackend-model-parity-fixture",
        Arc::new(js_model_capability("module.exports.default = {};")),
    );
    assert_eq!(
        native_missing
            .descriptor("chatbackend-model-parity-fixture")
            .unwrap()
            .available_models,
        js_missing
            .descriptor("chatbackend-model-parity-fixture")
            .unwrap()
            .available_models
    );

    let long_error = "native failure ".repeat(40);
    let native_error = ChatBackendRegistry::new();
    native_error.register_and_wait(
        "chatbackend-model-parity-fixture",
        Arc::new(ModelCapabilityNativeStub {
            outcome: Err(long_error),
        }),
    );
    let js_error = ChatBackendRegistry::new();
    js_error.register_and_wait(
        "chatbackend-model-parity-fixture",
        Arc::new(js_model_capability(
            r#"module.exports.default = {
                listModels() { throw new Error("js failure ".repeat(80)); }
            };"#,
        )),
    );
    for registry in [&native_error, &js_error] {
        assert!(registry
            .descriptor("chatbackend-model-parity-fixture")
            .unwrap()
            .available_models
            .is_empty());
        assert!(registry
            .model_capability_error("chatbackend-model-parity-fixture")
            .is_some_and(|error| error.chars().count() <= 256));
    }
}

#[tokio::test]
async fn discover_models_native_js_parity() {
    let native = DiscoverModelsNativeStub;
    let js = load_inline_js(
        "discover-fixture",
        r#"
            module.exports.default = {
                discoverModels() {
                    return [
                        { id: 'live-a', displayName: 'Live A', group: 'alpha', reasoningEfforts: [
                            { id: 'max', displayName: 'Max', description: 'Maximum reasoning' },
                            { id: 'ultra', displayName: 'Ultra' }
                        ] },
                        { id: 'live-b', displayName: 'Live B', description: 'second' },
                        { id: 'live-c', displayName: 'Live C', availability: 'unknown' }
                    ];
                }
            };
        "#,
    );

    assert_eq!(
        native.discover_models().await,
        Some(discover_models_fixture())
    );
    assert_eq!(js.discover_models().await, native.discover_models().await);

    let unknown = js
        .discover_models()
        .await
        .and_then(|models| models.into_iter().find(|m| m.id == "live-c"))
        .expect("unknown-verdict model crosses the JS boundary");
    assert_eq!(unknown.available, None);
    assert_eq!(
        unknown.availability(),
        crate::provider::ModelAvailability::Unknown,
        "a could-not-run verdict must not arrive as unavailable"
    );
}

#[test]
fn provider_readiness_native_js_parity() {
    use std::sync::Arc;

    let native = DiscoverModelsNativeStub;
    let manifest_json = r#"{
        "id": "provider-readiness-fixture",
        "version": "0.1.0",
        "displayName": "Provider readiness fixture",
        "spawn": {
            "readinessStrategy": {"kind": "poll_shell_prompt", "timeoutSecs": 30},
            "taskDelivery": "pty_input",
            "providerReadiness": {"kind": "structural_composer", "timeoutSecs": 17},
            "verifySubmit": {"kind": "none"}
        }
    }"#;
    let manifest: super::PluginManifest = serde_json::from_str(manifest_json).unwrap();
    let vm = super::PluginVm::from_source_with(
        "provider-readiness-fixture",
        "module.exports.default = {};".into(),
        Some(manifest_json.into()),
    )
    .unwrap();
    let js = JsProvider::new(manifest, Arc::new(vm));

    assert_eq!(
        js.provider_readiness_strategy(),
        native.provider_readiness_strategy()
    );
}

fn classify_tab_state_snapshot() -> crate::tab_state::TabStateSnapshot {
    crate::tab_state::TabStateSnapshot {
        tab_id: "tab-1".into(),
        agent_type: "classify-fixture".into(),
        session_id: None,
        cwd: None,
        screen_text: "Update available! 0.139.0 -> 0.141.0".into(),
    }
}

fn classify_tab_state_fixture() -> crate::tab_state::TabStateClassification {
    use crate::tab_state::*;
    TabStateClassification {
        state: TabState::UpdatePrompt,
        confidence: 1.0,
        data: None,
        action: Some(TabStateAction {
            keys: Some(vec!["down".into(), "enter".into()]),
            delayed_resume: None,
            surface: None,
        }),
    }
}

struct ClassifyNativeStub;

#[async_trait::async_trait]
impl AgentProvider for ClassifyNativeStub {
    fn id(&self) -> &str {
        "classify-fixture"
    }
    fn display_name(&self) -> &str {
        "Classify Fixture"
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> crate::provider::ReadinessStrategy {
        crate::provider::ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> crate::provider::TaskDelivery {
        crate::provider::TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<std::path::PathBuf> {
        None
    }
    fn detect_session_id(
        &self,
        _: &str,
        _: &[String],
        _: Option<std::time::Duration>,
    ) -> Option<String> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn classify_tab_state(
        &self,
        _: &crate::tab_state::TabStateSnapshot,
    ) -> Option<crate::tab_state::TabStateClassification> {
        Some(classify_tab_state_fixture())
    }

}

#[test]
fn classify_tab_state_native_js_parity() {
    let native = ClassifyNativeStub;
    let js = load_inline_js(
        "classify-fixture",
        r#"
            module.exports.default = {
                classifyTabState(snapshot) {
                    return {
                        state: 'update_prompt',
                        confidence: 1.0,
                        action: { keys: ['down', 'enter'] }
                    };
                }
            };
        "#,
    );
    let snap = classify_tab_state_snapshot();
    assert_eq!(
        native.classify_tab_state(&snap),
        Some(classify_tab_state_fixture())
    );
    assert_eq!(
        js.classify_tab_state(&snap),
        native.classify_tab_state(&snap)
    );
}

#[test]
fn classify_tab_state_unimplemented_matches_native_default_none() {
    // returns the SAME default (None) as the native trait default.
    let native_default = DiscoverModelsNativeStub; // does not override classify_tab_state
    let js = load_inline_js(
        "classify-none-fixture",
        r#"module.exports.default = { detectFromTitle() { return false; } };"#,
    );
    let snap = classify_tab_state_snapshot();
    assert_eq!(native_default.classify_tab_state(&snap), None);
    assert_eq!(js.classify_tab_state(&snap), None);
}

#[test]
fn host_toolcall_parse_native_js_parity() {
    let js = load_inline_js(
        "toolcall-fixture",
        r#"
            module.exports.default = {
                parse(raw, schema) {
                    return host.toolcall.parse(raw, schema);
                }
            };
        "#,
    );
    let schema = r#"{
        "tools":[{"name":"exec","args":["cmd"]}],
        "aliases":{"command":"cmd"}
    }"#;
    let raw = r#"Prose before.
```tool
{"tool":"exec","args":{"command":"ls"}}
```
Prose after."#;

    let via_js: String = super::call_json(
        js.vm(),
        "parse",
        &[serde_json::json!(raw), serde_json::json!(schema)],
    )
    .unwrap();
    let native = super::toolcall_parse::parse_to_json(raw, schema);
    assert_eq!(via_js, native);

    // SAME malformed signal through the JS binding as native (not a silent null).
    let malformed_raw = r#"{"tool":"exec","args":{"cmd":"ls"#;
    let via_js_malformed: String = super::call_json(
        js.vm(),
        "parse",
        &[serde_json::json!(malformed_raw), serde_json::json!(schema)],
    )
    .unwrap();
    let native_malformed = super::toolcall_parse::parse_to_json(malformed_raw, schema);
    assert_eq!(via_js_malformed, native_malformed);
    let parsed: serde_json::Value = serde_json::from_str(&via_js_malformed).unwrap();
    assert_eq!(parsed["status"], "malformed");
}

// Claude parity.

mod classifier;
mod claude;

// Loader behaviour.

#[test]
fn third_party_without_host_api_is_rejected_before_vm_load() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("unknown-third-party");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.json"),
        r#"{
            "id": "unknown-third-party",
            "version": "0.1.0",
            "displayName": "Unknown Third Party",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            },
            "entry": "plugin.js"
        }"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("plugin.js"),
        "throw new Error('must not execute before hostApi validation');",
    )
    .unwrap();

    let err = match load_one(&dir.join("plugin.json")) {
        Ok(_) => panic!("legacy drop-in must be skipped"),
        Err(err) => err,
    };
    assert!(err.contains("hostApi None unsupported"), "got: {err}");
    assert!(err.contains("skipping load"), "got: {err}");
}

#[test]
fn all_workspace_plugins_load() {
    if require_plugins().is_none() {
        return;
    }
    for id in ["claude", "codex", "opencode", "cursor"] {
        let p = load_js(id);
        assert_eq!(p.id(), id, "plugin id mismatch for {id}");
    }
}

#[test]
fn launch_adapter_matrix_uses_each_installed_descriptor() {
    let Some(root) = require_plugins() else {
        return;
    };
    let task = "adapter-task-fixture";
    let mut plugin_dirs = std::fs::read_dir(root)
        .expect("read bundled plugin directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("provider-"))
        .filter(|entry| entry.path().join("plugin.json").is_file())
        .collect::<Vec<_>>();
    plugin_dirs.sort_by_key(|entry| entry.file_name());

    assert!(
        !plugin_dirs.is_empty(),
        "installed descriptor matrix is empty"
    );
    for entry in plugin_dirs {
        let manifest = entry.path().join("plugin.json");
        let provider = load_one(&manifest)
            .unwrap_or_else(|error| panic!("load {}: {error}", manifest.display()));
        let descriptor = provider.describe();
        let command = provider.build_launch_command(&AgentLaunchParams {
            task: Some(task.into()),
            instance_id: Some("instance-parity".into()),
            ..Default::default()
        });

        assert!(
            !command.trim().is_empty(),
            "{} launch command",
            descriptor.id
        );
        let task_delivered_by_pty = matches!(
            provider.task_delivery(),
            crate::provider::TaskDelivery::PtyInput
        );
        match provider.task_delivery() {
            crate::provider::TaskDelivery::InlineFlag => {}
            crate::provider::TaskDelivery::PtyInput => {
                assert!(
                    !command.contains(task),
                    "{} PTY delivery must preserve first-message fallback",
                    descriptor.id
                );
            }
        }
        match provider.system_prompt_delivery() {
            crate::provider::SystemPromptDelivery::External if task_delivered_by_pty => {
                assert!(
                    !command.contains(task),
                    "{} external delivery must reserve confirmed fallback",
                    descriptor.id
                );
            }
            crate::provider::SystemPromptDelivery::External => {}
            crate::provider::SystemPromptDelivery::CliFlag(mechanism) => {
                let marker = mechanism
                    .split_whitespace()
                    .last()
                    .expect("CLI system prompt mechanism must not be empty");
                assert!(
                    command.contains(marker),
                    "{} system delivery must use declared mechanism {marker}",
                    descriptor.id
                );
                assert!(
                    !command.contains(task),
                    "{} system delivery must not use fallback text",
                    descriptor.id
                );
            }
            crate::provider::SystemPromptDelivery::SessionConfig => {
                assert!(
                    !command.contains(task),
                    "{} session config must not use fallback text",
                    descriptor.id
                );
            }
        }
    }
}

/// Structured-prompt-server capability contract across the bundled providers.
/// JS-only providers have no native counterpart to diff against, so the honest
/// "parity" is plugin-output-vs-documented-contract: opencode is the ONLY
/// provider whose structured handle is a host-bound SSE server, and the trait
/// documents `needs_structured_prompt_server() ⟹ supports_structured_prompts()`.
/// A manifest drift (dropping opencode's server flag, or setting it on a
/// pull-handle provider) would make the host bind the wrong transport.
#[test]
fn structured_prompt_server_capability_contract() {
    if require_plugins().is_none() {
        return;
    }
    for id in ["claude", "codex", "opencode", "cursor"] {
        let p = load_js(id);
        let needs_server = p.needs_structured_prompt_server();
        assert_eq!(
            needs_server,
            id == "opencode",
            "only opencode binds a host SSE server; {id} got needs_structured_prompt_server={needs_server}"
        );
        if needs_server {
            assert!(
                p.supports_structured_prompts(),
                "{id}: needs_structured_prompt_server implies supports_structured_prompts"
            );
        }
    }
}

#[test]
fn agent_state_status_reconciliation_capability_contract() {
    if require_plugins().is_none() {
        return;
    }
    for (id, expected) in [
        ("claude", true),
        ("codex", true),
        ("opencode", true),
        ("cursor", false),
    ] {
        assert_eq!(
            load_js(id).supports_agent_state_status_reconciliation(),
            expected,
            "{id}: capability must reflect deterministic prompt-ready and active-turn coverage"
        );
    }

    let native_default = DiscoverModelsNativeStub;
    let js_default = load_inline_js("agent-state-status-default", "module.exports.default = {};");
    assert!(!native_default.supports_agent_state_status_reconciliation());
    assert_eq!(
        native_default.supports_agent_state_status_reconciliation(),
        js_default.supports_agent_state_status_reconciliation()
    );
}

/// `model_launch_args` pins each provider's REAL CLI model flag — codex/opencode
/// take `-m`, claude/cursor take `--model`. A manifest edit that flips the flag
/// would launch the agent with a flag its CLI rejects; an empty model must add
/// no flag on every provider. Non-vacuous: it encodes the actual CLI contract,
/// not a tautology.
#[test]
fn model_launch_args_match_each_cli_flag_format() {
    if require_plugins().is_none() {
        return;
    }
    let cases = [
        ("claude", "--model"),
        ("codex", "-m"),
        ("cursor", "--model"),
        ("opencode", "-m"),
    ];
    for (id, flag) in cases {
        let p = load_js(id);
        assert!(
            p.model_launch_args("").is_empty(),
            "{id}: empty model must produce no launch flags"
        );
        assert_eq!(
            p.model_launch_args("some-model"),
            vec![flag.to_string(), "some-model".to_string()],
            "{id}: model flag format"
        );
    }
}

/// DN-2 read-preference parity: a stored transcript path whose filename
/// differs from the session id must be what BOTH the native and JS readers
/// resolve — neither may consult id-based reconstruction. The JS fixture's own
/// resolver is a reconstruction decoy: if the evidence preference were skipped
/// on the JS path, the decoy path would come back instead of the stored file.
#[test]
fn transcript_path_evidence_native_js_read_parity() {
    let tmp = tempfile::tempdir().unwrap();
    let evidence_dir = tmp.path().join("evidence");
    let _env = EnvGuard::new(&[(
        crate::session_evidence::EVIDENCE_DIR_ENV,
        evidence_dir.to_str().unwrap(),
    )]);

    let sid = "parity-evidence-sid-4aaa-8aaa-aaaaaaaaaaaa";
    let stored = tmp.path().join("vendor-named-differently.jsonl");
    std::fs::write(&stored, "{\"cwd\":\"x\"}\n").unwrap();
    assert!(crate::session_evidence::record(
        sid,
        stored.to_str().unwrap()
    ));

    let cwd = tmp.path().to_string_lossy().to_string();

    let native = crate::agent_claude::provider::ClaudeProvider::new();
    let native_path = native
        .session_file_path(&cwd, sid)
        .expect("native reader resolves the stored path");
    assert_eq!(native_path, stored, "native reader prefers the stored file");

    let js = load_inline_js(
        "transcript-evidence-fixture",
        r#"module.exports.default = {
            sessionFilePath(cwd, sessionId) { return cwd + "/" + sessionId + ".jsonl"; }
        };"#,
    );
    let js_path = js
        .session_file_path(&cwd, sid)
        .expect("js reader resolves the stored path");
    assert_eq!(
        js_path, native_path,
        "native and JS readers agree on the stored file"
    );
    assert_ne!(
        js_path,
        PathBuf::from(format!("{cwd}/{sid}.jsonl")),
        "the JS reconstruction decoy was not consulted"
    );
    assert_eq!(
        js.session_jsonl_path(&cwd, sid).as_deref(),
        Some(native_path.as_path()),
        "the chat-tail resolver honors the same stored path"
    );
    assert!(
        std::fs::read_to_string(&js_path).is_ok(),
        "both readers read the stored file itself"
    );

    // Absence control: no evidence ⇒ the JS resolver's own answer comes back.
    let plain_sid = "parity-no-evidence-sid";
    assert_eq!(
        js.session_file_path(&cwd, plain_sid),
        Some(PathBuf::from(format!("{cwd}/{plain_sid}.jsonl"))),
        "absence of evidence keeps today's resolution byte-for-byte"
    );
}

/// `supports_stop` (the UI Stop-affordance flag in the provider descriptor)
/// must stay in lockstep with `stop_keystroke().is_some()` for every bundled
/// provider — they are two reads of the same capability and a split would show
/// a Stop button that sends nothing (or hide one that works). This is the
/// drift class that hid the R1 native-Claude bug; here it guards the JS path.
/// NOTE: all four bundled manifests declare Esc as the stop key; the byte is a
/// manifest choice, so this test asserts the invariant rather than the raw byte.
#[test]
fn supports_stop_matches_stop_keystroke() {
    if require_plugins().is_none() {
        return;
    }
    for id in ["claude", "codex", "opencode", "cursor"] {
        let p = load_js(id);
        assert_eq!(
            p.describe().supports_stop,
            p.stop_keystroke().is_some(),
            "{id}: descriptor supports_stop must equal stop_keystroke().is_some()"
        );
    }
}

#[test]
fn permission_bypass_descriptor_matches_provider_capability() {
    if require_plugins().is_none() {
        return;
    }
    for id in ["claude", "codex", "opencode", "cursor"] {
        let provider = load_js(id);
        assert_eq!(
            provider.describe().supports_permission_bypass,
            !provider.auto_approve_flags().is_empty(),
            "{id}: UI bypass choice must be derived from provider capability"
        );
    }
}

// Hook event roster parity + fail-closed registration (DN-3 stage 1).

mod hook_event_roster;
mod integration_kind;
mod launch_artifacts;

/// #16 parity (discipline rule #2): a capability method a provider doesn't
/// implement must SKIP-not-recurse and return the SAME sane default on BOTH the
/// native and JS runtimes when there is no native fallback to delegate to. The
/// crash dump showed `JsProvider::native()`-delegation looping forever once a
/// JS-only provider became its own fallback; these two halves prove the JS path
/// now matches the native control instead of overflowing the stack.
mod no_native_fallback_recursion;

mod availability;
mod codex;
mod codex_launch_session;
mod context_owner;
mod cursor;
mod find_session;
mod manifest_launch_template;
mod model_groups;
mod reasoning_efforts;
mod opencode;
mod record_identity;
mod resume_first_turn;
mod trust_grant;
mod usage_sources;
