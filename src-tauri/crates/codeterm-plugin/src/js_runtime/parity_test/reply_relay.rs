use super::*;
use crate::provider::{ReplyRelayEvidence, ReplyRelayTurn};
use std::sync::Arc;

struct RelayNative;

#[test]
fn tool_less_class_native_js_parity_and_default() {
    let enabled = fixture_class(false, true, "module.exports.default = {};");
    assert_eq!(enabled.is_tool_less(), RelayNative.is_tool_less());
    let disabled = fixture(true, "module.exports.default = {};");
    assert_eq!(
        disabled.is_tool_less(),
        DiscoverModelsNativeStub.is_tool_less()
    );
    assert!(!disabled.is_tool_less());
    assert!(!enabled.supports_reply_relay());
}

fn turn() -> ReplyRelayTurn {
    ReplyRelayTurn {
        user_turn_id: "user-100".into(),
        user_record_start: 100,
        user_text: "opaque payload".into(),
        assistant_turn_id: "assistant-120".into(),
        answer: "final answer".into(),
        complete: true,
    }
}

#[async_trait::async_trait]
impl AgentProvider for RelayNative {
    fn id(&self) -> &str {
        "relay-native"
    }
    fn display_name(&self) -> &str {
        "Relay fixture"
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> crate::provider::ReadinessStrategy {
        DiscoverModelsNativeStub.readiness_strategy()
    }
    fn task_delivery(&self) -> crate::provider::TaskDelivery {
        DiscoverModelsNativeStub.task_delivery()
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
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
    fn is_tool_less(&self) -> bool {
        true
    }
    fn supports_reply_relay(&self) -> bool {
        true
    }
    fn read_reply_relay_turn(
        &self,
        cwd: &str,
        sid: &str,
        after: u64,
    ) -> Option<ReplyRelayEvidence> {
        (cwd == "/work" && sid == "own" && after <= 100)
            .then(|| ReplyRelayEvidence::Answered(turn()))
    }
}

fn fixture(enabled: bool, source: &str) -> JsProvider {
    fixture_class(enabled, false, source)
}

fn fixture_class(enabled: bool, tool_less: bool, source: &str) -> JsProvider {
    let manifest_json = serde_json::json!({
        "id": "relay-js", "version": "0.1.0", "displayName": "Relay fixture",
        "capabilities": { "agentProvider": true, "replyRelay": enabled, "toolLess": tool_less },
        "spawn": { "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
            "taskDelivery": "pty_input", "verifySubmit": { "kind": "none" } }
    })
    .to_string();
    let manifest = serde_json::from_str(&manifest_json).unwrap();
    let vm =
        super::super::PluginVm::from_source_with("relay-js", source.into(), Some(manifest_json))
            .unwrap();
    JsProvider::new(manifest, Arc::new(vm))
}

#[test]
fn reply_relay_capability_and_completed_evidence_native_js_parity() {
    let native = RelayNative;
    let js = fixture(
        true,
        r#"module.exports.default = {
        readReplyRelayTurn(cwd, sid, after) {
            return cwd === "/work" && sid === "own" && after <= 100
                ? { userTurnId: "user-100", userRecordStart: 100, userText: "opaque payload",
                    assistantTurnId: "assistant-120", answer: "final answer", complete: true } : null;
        }
    };"#,
    );
    assert_eq!(js.supports_reply_relay(), native.supports_reply_relay());
    for (cwd, sid, after) in [
        ("/work", "own", 100),
        ("/work", "other", 100),
        ("/other", "own", 100),
        ("/work", "own", 101),
    ] {
        assert_eq!(
            js.read_reply_relay_turn(cwd, sid, after),
            native.read_reply_relay_turn(cwd, sid, after)
        );
    }
}

#[test]
fn reply_relay_missing_method_or_capability_never_calls_the_vm_method() {
    let disabled = fixture(
        false,
        "module.exports.default = { readReplyRelayTurn() { throw new Error('must not call'); } };",
    );
    let absent = fixture(true, "module.exports.default = {};");
    for js in [disabled, absent] {
        assert!(!js.supports_reply_relay());
        assert_eq!(
            js.supports_reply_relay(),
            DiscoverModelsNativeStub.supports_reply_relay()
        );
        assert_eq!(
            js.read_reply_relay_turn("/work", "own", 0),
            DiscoverModelsNativeStub.read_reply_relay_turn("/work", "own", 0)
        );
    }
}

#[test]
fn reply_relay_null_and_malformed_evidence_remain_unknown() {
    for result in ["null", "{complete: true}"] {
        let source =
            format!("module.exports.default = {{ readReplyRelayTurn() {{ return {result}; }} }};");
        let js = fixture(true, &source);
        assert!(js.supports_reply_relay());
        assert_eq!(js.read_reply_relay_turn("/work", "own", 0), None);
    }
}

#[test]
fn reply_relay_non_success_and_unavailable_wire_contract() {
    for (result, expected) in [
        (
            r#"{userTurnId:"user-100",userRecordStart:100,userText:"opaque payload",complete:false,outcome:"error"}"#,
            ReplyRelayEvidence::Ended(crate::provider::ReplyRelayEndedTurn {
                user_turn_id: "user-100".into(),
                user_record_start: 100,
                user_text: "opaque payload".into(),
                complete: false,
                outcome: crate::provider::ReplyRelayOutcome::Error,
            }),
        ),
        (
            r#"{unavailable:true,reason:"adapter not active"}"#,
            ReplyRelayEvidence::Unavailable(crate::provider::ReplyRelayUnavailable {
                unavailable: true,
                reason: "adapter not active".into(),
            }),
        ),
    ] {
        let js = fixture(
            true,
            &format!("module.exports.default = {{ readReplyRelayTurn() {{ return {result}; }} }};"),
        );
        assert_eq!(
            js.read_reply_relay_turn("/work", "own", 100),
            Some(expected)
        );
    }
}

#[test]
fn reply_relay_invalid_discriminants_fail_closed() {
    for result in [
        r#"{unavailable:false,reason:"ignored"}"#,
        r#"{unavailable:true,reason:""}"#,
        r#"{userTurnId:"u",userRecordStart:1,userText:"x",complete:true,outcome:"error"}"#,
    ] {
        let js = fixture(
            true,
            &format!("module.exports.default = {{ readReplyRelayTurn() {{ return {result}; }} }};"),
        );
        assert_eq!(js.read_reply_relay_turn("/work", "own", 0), None);
    }
}
