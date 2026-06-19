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

use crate::agent::provider::{AgentLaunchParams, AgentProvider, ModelInfo};

use super::{load_one, JsProvider};

type PluginsDir = PathBuf;

fn workspace_plugins_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")).join("plugins")
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
        },
        ModelInfo {
            id: "live-b".into(),
            display_name: "Live B".into(),
            description: Some("second".into()),
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

    fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy {
        crate::agent::provider::ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::ZERO,
        }
    }

    fn task_delivery(&self) -> crate::agent::provider::TaskDelivery {
        crate::agent::provider::TaskDelivery::PtyInput
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

fn classify_pane_state_snapshot() -> crate::agent::pane_state::PaneStateSnapshot {
    crate::agent::pane_state::PaneStateSnapshot {
        pane_id: "pane-1".into(),
        agent_type: "classify-fixture".into(),
        session_id: None,
        cwd: None,
        screen_text: "Update available! 0.139.0 -> 0.141.0".into(),
    }
}

fn classify_pane_state_fixture() -> crate::agent::pane_state::PaneStateClassification {
    use crate::agent::pane_state::*;
    PaneStateClassification {
        state: PaneState::UpdatePrompt,
        confidence: 1.0,
        data: None,
        action: Some(PaneStateAction {
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
    fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy {
        crate::agent::provider::ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> crate::agent::provider::TaskDelivery {
        crate::agent::provider::TaskDelivery::PtyInput
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
    fn classify_pane_state(
        &self,
        _: &crate::agent::pane_state::PaneStateSnapshot,
    ) -> Option<crate::agent::pane_state::PaneStateClassification> {
        Some(classify_pane_state_fixture())
    }
}

#[test]
fn classify_pane_state_native_js_parity() {
    let native = ClassifyNativeStub;
    let js = load_inline_js(
        "classify-fixture",
        r#"
            module.exports.default = {
                classifyPaneState(snapshot) {
                    return {
                        state: 'update_prompt',
                        confidence: 1.0,
                        action: { keys: ['down', 'enter'] }
                    };
                }
            };
        "#,
    );
    let snap = classify_pane_state_snapshot();
    assert_eq!(
        native.classify_pane_state(&snap),
        Some(classify_pane_state_fixture())
    );
    assert_eq!(
        js.classify_pane_state(&snap),
        native.classify_pane_state(&snap)
    );
}

#[test]
fn classify_pane_state_unimplemented_matches_native_default_none() {
    // Discipline rule #2: a JS drop-in that doesn't export classifyPaneState
    // returns the SAME default (None) as the native trait default.
    let native_default = DiscoverModelsNativeStub; // does not override classify_pane_state
    let js = load_inline_js(
        "classify-none-fixture",
        r#"module.exports.default = { detectFromTitle() { return false; } };"#,
    );
    let snap = classify_pane_state_snapshot();
    assert_eq!(native_default.classify_pane_state(&snap), None);
    assert_eq!(js.classify_pane_state(&snap), None);
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

    // Tri-state parity: a tool-call-shaped but unparseable block must surface the
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

/// #16 parity (discipline rule #2): a capability method a provider doesn't
/// implement must SKIP-not-recurse and return the SAME sane default on BOTH the
/// native and JS runtimes when there is no native fallback to delegate to. The
/// crash dump showed `JsProvider::native()`-delegation looping forever once a
/// JS-only provider became its own fallback; these two halves prove the JS path
/// now matches the native control instead of overflowing the stack.
mod no_native_fallback_recursion;

// JS-only provider parity. Codex/cursor/opencode have NO native Rust
// counterpart, so "parity" here means: the real bundled plugin.js, driven
// through `JsProvider` (the exact trait surface core calls), produces output
// that round-trips into the native types — exercised + stable against
// realistic fixtures. Without these, a JS drop-in capability silently drifts.
mod codex;
mod cursor;
mod opencode;
