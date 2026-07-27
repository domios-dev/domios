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
    chatbackend_provider::{ChatBackendError, ChatBackendProvider},
    provider::{AgentLaunchParams, AgentProvider, ModelInfo},
};

use super::{load_one, JsProvider};

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

/// Serializes the (few) parity tests that drive a provider's filesystem
/// capabilities through process env overrides (`CODETERM_CODEX_SESSIONS_ROOT`,
/// `XDG_DATA_HOME`), so they can't observe each other's mutations under cargo's
/// parallel runner. Restores the prior values on drop.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    saved: Vec<(String, Option<String>)>,
}

impl EnvGuard {
    fn new(vars: &[(&str, &str)]) -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut saved = Vec::new();
        for (k, v) in vars {
            saved.push(((*k).to_string(), std::env::var(k).ok()));
            std::env::set_var(k, v);
        }
        Self { _lock: lock, saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, v) in &self.saved {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
    }
}

fn discover_models_fixture() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "live-a".into(),
            display_name: "Live A".into(),
            description: None,
            available: None,
        },
        ModelInfo {
            id: "live-b".into(),
            display_name: "Live B".into(),
            description: Some("second".into()),
            available: None,
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

    async fn discover_models(&self) -> Option<Vec<ModelInfo>> {
        Some(discover_models_fixture())
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

#[tokio::test]
async fn discover_models_native_js_parity() {
    let native = DiscoverModelsNativeStub;
    let js = load_inline_js(
        "discover-fixture",
        r#"
            module.exports.default = {
                discoverModels() {
                    return [
                        { id: 'live-a', displayName: 'Live A' },
                        { id: 'live-b', displayName: 'Live B', description: 'second' }
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

mod claude;
mod classifier;

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
            ..Default::default()
        });

        assert!(
            !command.trim().is_empty(),
            "{} launch command",
            descriptor.id
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
            crate::provider::SystemPromptDelivery::External => {
                assert!(
                    !command.contains(task),
                    "{} external delivery must reserve confirmed fallback",
                    descriptor.id
                );
            }
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

/// Reasoning-effort capability is declared in the manifest and surfaced through
/// the descriptor. codex declares low/medium/high with a scriptable `-c`
/// launch flag; providers that declare no efforts must expose no selector and
/// reject any requested effort structurally (no silent downgrade).
#[test]
fn reasoning_effort_capability_is_declared_and_gated() {
    if require_plugins().is_none() {
        return;
    }

    let codex = load_js("codex");
    let d = codex.describe();
    let ids: Vec<String> = d.reasoning_efforts.iter().map(|e| e.id.clone()).collect();
    assert_eq!(
        ids,
        vec!["low", "medium", "high"],
        "codex must surface its declared effort levels"
    );
    assert_eq!(d.default_reasoning_effort.as_deref(), Some("medium"));
    assert_eq!(
        codex.reasoning_effort_launch_args("high"),
        vec![
            "-c".to_string(),
            "model_reasoning_effort=\"high\"".to_string()
        ],
        "codex effort launch flag must match its CLI config-override contract"
    );
    assert!(
        codex.reasoning_effort_launch_args("").is_empty(),
        "empty effort must produce no launch flags"
    );
    assert!(codex.supports_reasoning_effort("medium"));
    assert!(codex.validate_reasoning_effort(Some("high")).is_ok());
    let rejection = codex
        .validate_reasoning_effort(Some("ultra"))
        .expect_err("unsupported effort must be rejected");
    assert_eq!(rejection.requested, "ultra");
    assert_eq!(rejection.supported, vec!["low", "medium", "high"]);

    // A provider that declares no efforts: no selector, and any effort rejects.
    let claude = load_js("claude");
    assert!(
        claude.describe().reasoning_efforts.is_empty(),
        "claude declares no efforts ⇒ no selector"
    );
    assert!(
        claude.reasoning_effort_launch_args("high").is_empty(),
        "no-capability provider adds no effort flags"
    );
    assert!(claude.validate_reasoning_effort(None).is_ok());
    assert!(
        claude.validate_reasoning_effort(Some("high")).is_err(),
        "no-capability provider must reject an explicit effort, not silently ignore it"
    );
}

/// `supports_stop` (the UI Stop-affordance flag in the provider descriptor)
/// must stay in lockstep with `stop_keystroke().is_some()` for every bundled
/// provider — they are two reads of the same capability and a split would show
/// a Stop button that sends nothing (or hide one that works). This is the
/// drift class that hid the R1 native-Claude bug; here it guards the JS path.
/// NOTE: all four bundled manifests currently declare Esc as the stop key — see
/// the loop report for the cursor doc/manifest discrepancy (trait doc says
/// cursor-agent listens for Ctrl+C) which needs live-CLI verification, not a
/// blind patch, so this test asserts the invariant rather than the raw byte.
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

/// #16 parity (discipline rule #2): a capability method a provider doesn't
/// implement must SKIP-not-recurse and return the SAME sane default on BOTH the
/// native and JS runtimes when there is no native fallback to delegate to. The
/// crash dump showed `JsProvider::native()`-delegation looping forever once a
/// JS-only provider became its own fallback; these two halves prove the JS path
/// now matches the native control instead of overflowing the stack.
mod no_native_fallback_recursion;

mod codex;
mod cursor;
mod opencode;
