//! Sidecar assertions read the decision file the host wrote — never a
//! rendering. Every backend id is synthetic.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::*;
use crate::agent::chat_types::{ChatMsg, StructuredChat};
use crate::chatbackend::{ChatBackendError, ChatBackendProvider, HostTurn};
use crate::agent::provider::{ContextOwner, ModelInfo};

struct RecordingBackend {
    owner: ContextOwner,
    turns: Mutex<Vec<(String, HostTurn)>>,
    messages: Mutex<Vec<(String, String)>>,
    send_turn_result: Result<(), &'static str>,
}

impl RecordingBackend {
    fn new(owner: ContextOwner) -> Self {
        Self {
            owner,
            turns: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
            send_turn_result: Ok(()),
        }
    }

    fn rejecting() -> Self {
        Self {
            send_turn_result: Err("model rejected the payload"),
            ..Self::new(ContextOwner::Host)
        }
    }
}

impl ChatBackendProvider for RecordingBackend {
    fn id(&self) -> &str {
        "host-context-fixture"
    }

    fn context_owner(&self) -> ContextOwner {
        self.owner
    }

    fn supports_send_turn(&self) -> bool {
        true
    }

    fn send_turn(&self, session_id: &str, turn: HostTurn) -> Result<(), ChatBackendError> {
        self.turns
            .lock()
            .unwrap()
            .push((session_id.to_string(), turn));
        self.send_turn_result
            .map_err(|error| ChatBackendError::Call(error.into()))
    }

    fn open_session(&self, _: serde_json::Value) -> Result<String, ChatBackendError> {
        unreachable!("the delivery fixture does not open sessions")
    }

    fn send_message(&self, session_id: &str, text: &str) -> Result<(), ChatBackendError> {
        self.messages
            .lock()
            .unwrap()
            .push((session_id.to_string(), text.to_string()));
        Ok(())
    }

    fn poll(&self, _: &str, _: Option<&str>) -> Result<StructuredChat, ChatBackendError> {
        unreachable!("the delivery fixture does not poll sessions")
    }

    fn close_session(&self, _: &str) -> Result<(), ChatBackendError> {
        unreachable!("the delivery fixture does not close sessions")
    }

    fn list_models(&self) -> Result<Vec<ModelInfo>, ChatBackendError> {
        Ok(Vec::new())
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

/// `groups` whole turn groups plus a `_meta` role brief, written the way the
/// poll-side mirror writes them.
fn seed_mirror(path: &Path, brief: &str, groups: usize) {
    let mut history = chatbackend_history::ChatBackendHistory::open_at(path.to_path_buf());
    history.ensure_meta(&serde_json::json!({ "systemPrompt": brief }));
    let mut messages = Vec::new();
    for index in 0..groups {
        messages.push(entry(&format!("user-{index}"), "user", "ask"));
        messages.push(entry(&format!("assistant-{index}"), "assistant", "answer"));
    }
    history.upsert(&messages);
}

struct Session {
    _dir: tempfile::TempDir,
    mirror: PathBuf,
    sidecar: PathBuf,
}

fn session() -> Session {
    let dir = tempfile::tempdir().unwrap();
    let mirror = dir.path().join("s.jsonl");
    let sidecar = dir.path().join("s.context.jsonl");
    Session {
        _dir: dir,
        mirror,
        sidecar,
    }
}

#[test]
fn context_decision_line_records_the_limit_in_effect() {
    let s = session();
    let brief = "You are CodeTerm.";
    // One more group than the bound allows, so the limit is observably doing something.
    let groups = context_window::PROVISIONAL_GROUP_LIMIT + 3;
    seed_mirror(&s.mirror, brief, groups);
    let backend = RecordingBackend::new(ContextOwner::Host);

    deliver_host_turn(&backend, "session-1", &s.mirror, &s.sidecar, "next ask").unwrap();

    let decisions = read_decisions(&s.sidecar);
    assert_eq!(decisions.len(), 1, "one line per host-composed turn");
    let decision = &decisions[0];
    assert_eq!(decision.turn, 1);
    assert_eq!(decision.unit, context_window::ContextUnit::Messages);
    assert_eq!(decision.limit, context_window::PROVISIONAL_GROUP_LIMIT);
    assert_eq!(decision.groups_available, groups + 1);
    assert_eq!(decision.groups_sent, context_window::PROVISIONAL_GROUP_LIMIT);
    assert_eq!(
        decision.groups_dropped,
        groups + 1 - context_window::PROVISIONAL_GROUP_LIMIT
    );
    assert!(decision.system_prompt_included);
    assert_eq!(decision.system_prompt_bytes, brief.len());
    assert_eq!(decision.outcome, context_window::ContextOutcome::Sent);
    assert!(
        decision.dropped_entry_ids.contains(&"user-0".to_string()),
        "the dropped entries are named: {:?}",
        decision.dropped_entry_ids
    );

    let (session_id, turn) = backend.turns.lock().unwrap()[0].clone();
    assert_eq!(session_id, "session-1");
    assert_eq!(turn.system, brief, "the role brief crossed the boundary");
    assert_eq!(turn.groups.len(), decision.groups_sent);
    assert_eq!(
        turn.groups.last().unwrap().entries[0].content,
        "next ask",
        "the user turn's content is passed through verbatim"
    );
}

#[test]
fn each_host_composed_turn_appends_one_numbered_line() {
    let s = session();
    seed_mirror(&s.mirror, "brief", 2);
    let backend = RecordingBackend::new(ContextOwner::Host);

    deliver_host_turn(&backend, "session-1", &s.mirror, &s.sidecar, "one").unwrap();
    deliver_host_turn(&backend, "session-1", &s.mirror, &s.sidecar, "two").unwrap();

    let turns: Vec<u64> = read_decisions(&s.sidecar)
        .iter()
        .map(|decision| decision.turn)
        .collect();
    assert_eq!(turns, vec![1, 2]);
}

#[test]
fn a_rejected_payload_is_recorded_loudly_and_never_retried_smaller() {
    let s = session();
    seed_mirror(&s.mirror, "brief", 3);
    let backend = RecordingBackend::rejecting();

    let error = deliver_host_turn(&backend, "session-1", &s.mirror, &s.sidecar, "ask").unwrap_err();
    assert!(error.contains("model rejected the payload"));

    let decisions = read_decisions(&s.sidecar);
    assert_eq!(decisions.len(), 1, "one attempt, one line — no auto-retry");
    assert_eq!(
        decisions[0].outcome,
        context_window::ContextOutcome::Rejected
    );
    assert!(decisions[0].error.is_some());
    assert_eq!(
        backend.turns.lock().unwrap().len(),
        1,
        "the host never shrinks the window and sends again"
    );
}

#[test]
fn an_unreadable_mirror_degrades_loudly_not_silently() {
    let s = session();
    // A directory where the mirror file should be: present, unreadable as text.
    std::fs::create_dir(&s.mirror).unwrap();
    let backend = RecordingBackend::new(ContextOwner::Host);

    deliver_host_turn(&backend, "session-1", &s.mirror, &s.sidecar, "ask").unwrap();

    let decisions = read_decisions(&s.sidecar);
    assert_eq!(
        decisions[0].outcome,
        context_window::ContextOutcome::TranscriptUnavailable,
        "a context-free turn is recorded as such, never sent quietly"
    );
    assert_eq!(decisions[0].groups_sent, 1);
}

#[test]
fn an_oversized_payload_refuses_the_send_and_records_the_refusal() {
    let s = session();
    let mut history = chatbackend_history::ChatBackendHistory::open_at(s.mirror.clone());
    history.upsert(&[
        entry("user-0", "user", "ask"),
        entry(
            "assistant-0",
            "assistant",
            &"x".repeat(context_window::MAX_PAYLOAD_BYTES + 1),
        ),
    ]);
    let backend = RecordingBackend::new(ContextOwner::Host);

    let error = deliver_host_turn(&backend, "session-1", &s.mirror, &s.sidecar, "ask").unwrap_err();
    let error: serde_json::Value = serde_json::from_str(&error).unwrap();
    assert_eq!(error["code"], PAYLOAD_TOO_LARGE_CODE);
    assert_eq!(error["entryId"], "assistant-0");

    assert!(
        backend.turns.lock().unwrap().is_empty(),
        "nothing was sent and nothing was clipped"
    );
    assert_eq!(
        read_decisions(&s.sidecar)[0].outcome,
        context_window::ContextOutcome::PayloadTooLarge
    );
}

#[test]
fn default_path_writes_no_context_sidecar() {
    let plugin = "default-path-no-sidecar-fixture";
    let session_id = "plugin-owned-session";
    let mirror = chatbackend_history::session_path(plugin, session_id);
    let sidecar = chatbackend_history::context_sidecar_path(plugin, session_id);
    let _ = std::fs::remove_file(&mirror);
    let _ = std::fs::remove_file(&sidecar);
    seed_mirror(&mirror, "brief", 3);
    let before = std::fs::read(&mirror).unwrap();

    let backend = RecordingBackend::new(ContextOwner::Plugin);
    crate::service::tab_transport_chat::dispatch_chatbackend_send(
        &backend,
        plugin,
        session_id,
        "ask",
    )
    .unwrap();

    assert_eq!(
        backend.messages.lock().unwrap().as_slice(),
        &[(session_id.to_string(), "ask".to_string())],
        "the provider-owned path is the unchanged send_message call"
    );
    assert!(
        backend.turns.lock().unwrap().is_empty(),
        "send_turn is never reached on the default path"
    );
    assert!(
        !sidecar.exists(),
        "a plugin-owned session writes no decision sidecar at all"
    );
    assert_eq!(
        std::fs::read(&mirror).unwrap(),
        before,
        "the mirror JSONL is byte-identical across the send"
    );

    let _ = std::fs::remove_file(&mirror);
    let _ = std::fs::remove_file(&sidecar);
}

#[test]
fn host_owned_send_writes_the_sidecar_beside_the_mirror() {
    let plugin = "host-path-sidecar-fixture";
    let session_id = "host-owned-session";
    let mirror = chatbackend_history::session_path(plugin, session_id);
    let sidecar = chatbackend_history::context_sidecar_path(plugin, session_id);
    let _ = std::fs::remove_file(&mirror);
    let _ = std::fs::remove_file(&sidecar);
    seed_mirror(&mirror, "brief", 2);
    let before = std::fs::read(&mirror).unwrap();

    let backend = RecordingBackend::new(ContextOwner::Host);
    crate::service::tab_transport_chat::dispatch_chatbackend_send(
        &backend,
        plugin,
        session_id,
        "ask",
    )
    .unwrap();

    assert_eq!(mirror.parent(), sidecar.parent(), "sidecar sits beside the mirror");
    assert_eq!(read_decisions(&sidecar).len(), 1);
    assert_eq!(
        std::fs::read(&mirror).unwrap(),
        before,
        "the send side never writes the poll-side mirror"
    );

    let _ = std::fs::remove_file(&mirror);
    let _ = std::fs::remove_file(&sidecar);
}
