//! `contextOwner` capability parity: a JS drop-in declaring the manifest key and
//! a native backend declaring the same ownership must agree on the descriptor
//! field and on what `send_turn` carries across the boundary. The third case is
//! the control — no PTY provider's descriptor moves because the field exists.

use std::sync::Arc;
use std::sync::Mutex;

use codeterm_types::agent::chat_types::{ChatMsg, StructuredChat};

use crate::chatbackend_provider::{ChatBackendError, ChatBackendProvider, HostTurn, TurnGroup};
use crate::provider::{AgentProvider, ContextOwner, ModelInfo, ProviderTransport};

use super::{require_plugins, workspace_plugins_root};

fn js_chat_backend(
    id: &str,
    manifest_json: &str,
    plugin_js: &str,
) -> crate::js_runtime::JsChatBackend {
    let manifest: crate::js_runtime::PluginManifest = serde_json::from_str(manifest_json).unwrap();
    let vm = crate::js_runtime::PluginVm::from_source_with(
        id,
        plugin_js.to_string(),
        Some(manifest_json.to_string()),
    )
    .expect("fixture vm");
    crate::js_runtime::JsChatBackend::from_manifest(Arc::new(vm), &manifest)
}

/// The composed turn both runtimes receive. The role brief is its own field, so
/// a group slice can never reach it.
fn host_turn_fixture() -> HostTurn {
    HostTurn {
        system: "fixture role brief".into(),
        groups: vec![
            TurnGroup {
                entries: vec![
                    entry("fixture-user-1", "user", "first ask"),
                    entry("fixture-assistant-1", "assistant", "first answer"),
                ],
            },
            TurnGroup {
                entries: vec![
                    entry("fixture-user-2", "user", "second ask"),
                    entry("fixture-tool-call-1", "tool_call", "call"),
                    entry("fixture-tool-result-1", "tool_result", "result"),
                ],
            },
        ],
    }
}

fn entry(id: &str, kind: &str, content: &str) -> ChatMsg {
    ChatMsg {
        id: id.into(),
        kind: kind.into(),
        content: content.into(),
        ..ChatMsg::default()
    }
}

struct ContextOwnerNativeStub {
    id: &'static str,
    owner: ContextOwner,
    implements_send_turn: bool,
    received: Mutex<Vec<(String, HostTurn)>>,
}

impl ContextOwnerNativeStub {
    fn new(id: &'static str, owner: ContextOwner) -> Self {
        Self {
            id,
            owner,
            implements_send_turn: true,
            received: Mutex::new(Vec::new()),
        }
    }

    fn without_send_turn(id: &'static str) -> Self {
        Self {
            implements_send_turn: false,
            ..Self::new(id, ContextOwner::Host)
        }
    }
}

impl ChatBackendProvider for ContextOwnerNativeStub {
    fn id(&self) -> &str {
        self.id
    }

    fn context_owner(&self) -> ContextOwner {
        self.owner
    }

    fn supports_send_turn(&self) -> bool {
        self.implements_send_turn
    }

    fn send_turn(&self, session_id: &str, turn: HostTurn) -> Result<(), ChatBackendError> {
        self.received
            .lock()
            .unwrap()
            .push((session_id.to_string(), turn));
        Ok(())
    }

    fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
        unreachable!("context-owner parity does not open a session")
    }

    fn send_message(&self, _: &str, _: &str) -> Result<(), ChatBackendError> {
        unreachable!("context-owner parity never takes the plugin-owned path")
    }

    fn poll(&self, _: &str, _: Option<&str>) -> Result<StructuredChat, ChatBackendError> {
        unreachable!("context-owner parity does not poll sessions")
    }

    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        unreachable!("context-owner parity does not close sessions")
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
        unreachable!("context-owner parity does not list models")
    }
}

#[test]
fn chatbackend_context_owner_descriptor_native_js_parity() {
    let host_js = js_chat_backend(
        "context-owner-host-fixture",
        r#"{
            "id":"context-owner-host-fixture",
            "version":"0.1.0",
            "displayName":"Context Owner Host Fixture",
            "capabilities":{"chatBackend":{"contextOwner":"host"}}
        }"#,
        "module.exports.default = {};",
    );
    let host_native = ContextOwnerNativeStub::new("context-owner-host-fixture", ContextOwner::Host);

    assert_eq!(
        host_js.describe().context_owner,
        host_native.describe().context_owner,
        "a declared host owner must reach the descriptor identically on both runtimes"
    );
    assert_eq!(host_js.describe().context_owner, ContextOwner::Host);

    let absent_js = js_chat_backend(
        "context-owner-absent-fixture",
        r#"{
            "id":"context-owner-absent-fixture",
            "version":"0.1.0",
            "displayName":"Context Owner Absent Fixture",
            "capabilities":{"chatBackend":{"spawnable":true}}
        }"#,
        "module.exports.default = {};",
    );
    let absent_native =
        ContextOwnerNativeStub::new("context-owner-absent-fixture", ContextOwner::Plugin);

    assert_eq!(
        absent_js.describe().context_owner,
        absent_native.describe().context_owner,
        "an absent key must mean provider-owned on both runtimes"
    );
    assert_eq!(absent_js.describe().context_owner, ContextOwner::Plugin);
}

#[test]
fn chatbackend_send_turn_native_js_parity() {
    let turn = host_turn_fixture();

    let js = js_chat_backend(
        "context-owner-send-turn-fixture",
        r#"{
            "id":"context-owner-send-turn-fixture",
            "version":"0.1.0",
            "displayName":"Send Turn Fixture",
            "capabilities":{"chatBackend":{"contextOwner":"host"}}
        }"#,
        r#"let received = "";
           module.exports.default = {
             sendTurn(sid, turn) { received = JSON.stringify({ sessionId: sid, turn }); },
             poll() { return [{ id: "1", type: "assistant", content: received }]; }
           }"#,
    );
    let native = ContextOwnerNativeStub::new("context-owner-send-turn-fixture", ContextOwner::Host);

    js.send_turn("parity-session", turn.clone()).unwrap();
    native.send_turn("parity-session", turn.clone()).unwrap();

    let js_recorded: serde_json::Value =
        serde_json::from_str(&js.poll("parity-session", None).unwrap().messages[0].content)
            .expect("JS fixture recorded a JSON turn");
    let (native_session, native_turn) = {
        let received = native.received.lock().unwrap();
        assert_eq!(received.len(), 1, "the native stub took exactly one turn");
        (received[0].0.clone(), received[0].1.clone())
    };
    let native_recorded = serde_json::json!({
        "sessionId": native_session,
        "turn": serde_json::to_value(&native_turn).unwrap(),
    });

    assert_eq!(
        js_recorded, native_recorded,
        "the JS adapter must forward the composed turn unmodified"
    );
    assert_eq!(
        js_recorded["turn"],
        serde_json::to_value(&turn).unwrap(),
        "both runtimes must receive the turn the host composed"
    );
    assert_eq!(
        js_recorded["turn"]["system"], "fixture role brief",
        "the role brief travels in its own field, outside the sliced collection"
    );
}

/// The probe the open-time refusal reads: both runtimes must agree on whether a
/// backend declaring host ownership actually implements the host-turn entry
/// point, so neither can be opened and then silently downgraded at send time.
#[test]
fn chatbackend_send_turn_export_probe_native_js_parity() {
    let manifest = r#"{
        "id":"context-owner-probe-fixture",
        "version":"0.1.0",
        "displayName":"Send Turn Probe Fixture",
        "capabilities":{"chatBackend":{"contextOwner":"host"}}
    }"#;

    let exporting = js_chat_backend(
        "context-owner-probe-fixture",
        manifest,
        "module.exports.default = { sendTurn(sid, turn) {} };",
    );
    let exporting_native =
        ContextOwnerNativeStub::new("context-owner-probe-fixture", ContextOwner::Host);
    assert_eq!(
        exporting.supports_send_turn(),
        exporting_native.supports_send_turn(),
        "an implemented host-turn entry point reads the same on both runtimes"
    );
    assert!(exporting.supports_send_turn());

    let missing = js_chat_backend(
        "context-owner-probe-fixture",
        manifest,
        "module.exports.default = {};",
    );
    let missing_native = ContextOwnerNativeStub::without_send_turn("context-owner-probe-fixture");
    assert_eq!(
        missing.supports_send_turn(),
        missing_native.supports_send_turn(),
        "a missing host-turn entry point reads the same on both runtimes"
    );
    assert!(!missing.supports_send_turn());
}

/// The field exists on every descriptor, so the control asserts it costs a PTY
/// provider nothing: `Plugin` for all of them and no bytes added to the wire.
#[test]
fn pty_provider_descriptors_unchanged_by_context_owner_field() {
    let native = NativePtyStub;
    assert_pty_descriptor_unchanged("native-pty-fixture", &native);

    if require_plugins().is_none() {
        return;
    }
    let mut bundled = 0usize;
    for bundle in std::fs::read_dir(workspace_plugins_root()).expect("plugins dir") {
        let dir = bundle.expect("plugins dir entry").path();
        let is_provider_bundle = dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("provider-"));
        if !is_provider_bundle {
            continue;
        }
        let manifest = dir.join("plugin.json");
        let provider = super::load_one(&manifest)
            .unwrap_or_else(|e| panic!("load {}: {e}", manifest.display()));
        assert_pty_descriptor_unchanged(&dir.display().to_string(), &provider);
        bundled += 1;
    }
    assert!(
        bundled > 0,
        "no bundled PTY provider was checked — the control must not pass vacuously"
    );
}

fn assert_pty_descriptor_unchanged(source: &str, provider: &dyn AgentProvider) {
    let descriptor = provider.describe();
    assert_eq!(
        descriptor.transport,
        ProviderTransport::Pty,
        "{source}: control covers PTY providers"
    );
    assert_eq!(
        descriptor.context_owner,
        ContextOwner::Plugin,
        "{source}: a PTY provider is provider-owned"
    );

    let wire = serde_json::to_value(&descriptor).expect("serialize descriptor");
    assert!(
        wire.get("context_owner").is_none(),
        "{source}: a provider-owned descriptor must add no bytes to the wire"
    );
    let decoded: crate::provider::ProviderDescriptor =
        serde_json::from_value(wire).expect("descriptor round-trip");
    assert_eq!(
        decoded, descriptor,
        "{source}: no other descriptor field moved"
    );
}

struct NativePtyStub;

#[async_trait::async_trait]
impl AgentProvider for NativePtyStub {
    fn id(&self) -> &str {
        "native-pty-fixture"
    }

    fn display_name(&self) -> &str {
        "Native PTY fixture"
    }

    fn build_launch_command(&self, _: &crate::provider::AgentLaunchParams) -> String {
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
}
