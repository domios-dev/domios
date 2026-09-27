//! Closed-chat evidence reconciliation for Chat sends.
//!
//! A send whose live delta stream was not running (Chat closed during
//! delivery) can still be classified honestly: each send captures its bound
//! session and a safe [`SendEvidenceAnchor`] when one exists. Ledger reads
//! inspect ONLY that session's bounded append window. Pre-send history, other
//! sessions, and already-used turns can never confirm a send.

use crate::agent::registry::AgentRegistry;
use crate::service::chat_send_transaction::{
    normalize_turn_evidence, ChatSendLedger, ChatSendStatus, EvidenceWindow, SendEvidenceAnchor,
    SendLiveBinding,
};
use crate::service::ServiceContext;

fn window_user_turns(registry: &AgentRegistry, anchor: &SendEvidenceAnchor) -> Vec<String> {
    match &anchor.window {
        EvidenceWindow::Jsonl { transcript_offset } => {
            let Some(path) = registry
                .with_provider(&anchor.provider_id, |provider| {
                    provider.session_jsonl_path(&anchor.cwd, &anchor.session_id)
                })
                .flatten()
            else {
                return Vec::new();
            };
            if !path.exists() {
                return Vec::new();
            }
            registry
                .with_provider(&anchor.provider_id, |provider| {
                    provider.parse_session_delta(&path, *transcript_offset)
                })
                .map(|delta| {
                    delta
                        .messages
                        .iter()
                        .enumerate()
                        .flat_map(|(index, rich)| {
                            crate::service::tab_chat::rich_to_chat_msgs(rich, index)
                        })
                        .filter(|msg| msg.kind == "user")
                        .map(|msg| msg.content)
                        .collect()
                })
                .unwrap_or_default()
        }
        EvidenceWindow::Structured { cursor } => registry
            .with_provider(&anchor.provider_id, |provider| {
                provider.read_structured_chat(&anchor.cwd, &anchor.session_id, cursor.as_deref())
            })
            .map(|chat| {
                chat.messages
                    .into_iter()
                    .filter(|msg| msg.kind == "user")
                    .map(|msg| msg.content)
                    .collect()
            })
            .unwrap_or_default(),
    }
}

#[cfg(test)]
pub(crate) fn contains_exact_user_turn(
    registry: &AgentRegistry,
    anchor: &SendEvidenceAnchor,
    payload: &str,
) -> bool {
    let expected = normalize_turn_evidence(payload);
    !expected.is_empty()
        && window_user_turns(registry, anchor)
            .iter()
            .any(|turn| normalize_turn_evidence(turn) == expected)
}

#[derive(Default)]
pub(crate) struct ExactTurnCursor {
    jsonl_offset: Option<u64>,
    confirmed: bool,
}

impl ExactTurnCursor {
    pub(crate) fn for_anchor(anchor: Option<&SendEvidenceAnchor>) -> Self {
        let mut cursor = Self::default();
        if let Some(SendEvidenceAnchor {
            window: EvidenceWindow::Jsonl { transcript_offset },
            ..
        }) = anchor
        {
            cursor.jsonl_offset = Some(*transcript_offset);
        }
        cursor
    }
}

pub(crate) fn contains_exact_user_turn_since(
    registry: &AgentRegistry,
    anchor: &SendEvidenceAnchor,
    payload: &str,
    cursor: &mut ExactTurnCursor,
) -> bool {
    if cursor.confirmed {
        return true;
    }
    let expected = normalize_turn_evidence(payload);
    if expected.is_empty() {
        return false;
    }
    let found = match &anchor.window {
        EvidenceWindow::Jsonl { transcript_offset } => {
            let Some(path) = registry
                .with_provider(&anchor.provider_id, |provider| {
                    provider.session_jsonl_path(&anchor.cwd, &anchor.session_id)
                })
                .flatten()
            else {
                return false;
            };
            let from_offset = cursor.jsonl_offset.unwrap_or(*transcript_offset);
            let Some(delta) = registry
                .with_provider(&anchor.provider_id, |provider| {
                    provider.parse_session_delta(&path, from_offset)
                })
            else {
                return false;
            };
            cursor.jsonl_offset = Some(delta.new_offset.max(from_offset));
            delta.messages.iter().enumerate().any(|(index, rich)| {
                crate::service::tab_chat::rich_to_chat_msgs(rich, index)
                    .iter()
                    .any(|message| {
                        message.kind == "user"
                            && normalize_turn_evidence(&message.content) == expected
                    })
            })
        }
        EvidenceWindow::Structured { .. } => window_user_turns(registry, anchor)
            .iter()
            .any(|turn| normalize_turn_evidence(turn) == expected),
    };
    cursor.confirmed = found;
    found
}

/// A cursor-less NON-empty structured store has no bounded append window and
/// stays live-sink-only.
pub(crate) fn structured_boundary(
    chat: &crate::agent::chat_types::StructuredChat,
) -> Option<EvidenceWindow> {
    if chat.cursor.is_none() && !chat.messages.is_empty() {
        return None;
    }
    Some(EvidenceWindow::Structured {
        cursor: chat.cursor.clone(),
    })
}

fn jsonl_boundary(path: Option<std::path::PathBuf>) -> Option<EvidenceWindow> {
    let transcript_offset = std::fs::metadata(path?).ok()?.len();
    Some(EvidenceWindow::Jsonl { transcript_offset })
}

#[derive(Clone, Default)]
pub(crate) struct CapturedSendEvidence {
    pub(crate) anchor: Option<SendEvidenceAnchor>,
    pub(crate) live_binding: Option<SendLiveBinding>,
}

/// One turn instance confirms at most one send: a scan may only confirm when
/// the append window holds MORE matching user turns than sends already
/// confirmed for the same tab+session+payload.
pub(crate) fn scan_transcript_evidence(
    registry: &AgentRegistry,
    ledger: &ChatSendLedger,
    tab_id: &str,
    mut on_confirmed: impl FnMut(ChatSendStatus),
) {
    for candidate in ledger.evidence_candidates(tab_id) {
        let anchor = &candidate.anchor;
        let matching_turns = window_user_turns(registry, anchor)
            .iter()
            .filter(|content| normalize_turn_evidence(content) == candidate.payload)
            .count();
        let claimed = ledger.claimed_turn_count(tab_id, &anchor.session_id, &candidate.payload);
        if matching_turns > claimed {
            if let Some(status) = ledger.confirm_send(&candidate.send_id) {
                on_confirmed(status);
            }
        }
    }
}

impl ServiceContext {
    /// Capture the bound session plus its safe append-window anchor, if one
    /// exists. An unbounded or not-yet-materialized store stays live-sink-only.
    pub(crate) fn chat_send_anchor(&self, tab_id: &str) -> CapturedSendEvidence {
        let captured = {
            let mux = self.mux.read();
            mux.get_tab(tab_id).map(|tab| {
                (
                    tab.agent_type.clone(),
                    tab.cwd.clone().unwrap_or_default(),
                    tab.agent_session_id.clone(),
                    tab.session_binding_nonce.clone(),
                )
            })
        };
        let Some((provider_id, cwd, session_id, binding_nonce)) = captured else {
            return CapturedSendEvidence::default();
        };
        let live_binding =
            (session_id.is_some() || binding_nonce.is_some()).then(|| SendLiveBinding {
                session_id: session_id.clone(),
                binding_nonce: binding_nonce.clone(),
            });
        let (Some(provider_id), Some(session_id)) = (provider_id, session_id) else {
            return CapturedSendEvidence {
                anchor: None,
                live_binding,
            };
        };
        let structured = self
            .agent_registry
            .with_provider(&provider_id, |provider| provider.uses_structured_chat())
            .unwrap_or(false);
        let window = if structured {
            let chat = self
                .agent_registry
                .with_provider(&provider_id, |provider| {
                    provider.read_structured_chat(&cwd, &session_id, None)
                })
                .unwrap_or_default();
            structured_boundary(&chat)
        } else {
            let path = self
                .agent_registry
                .with_provider(&provider_id, |provider| {
                    provider.session_jsonl_path(&cwd, &session_id)
                })
                .flatten();
            jsonl_boundary(path)
        };
        let anchor = window.map(|window| SendEvidenceAnchor {
            provider_id,
            cwd,
            session_id: session_id.clone(),
            window,
        });
        CapturedSendEvidence {
            anchor,
            live_binding,
        }
    }

    /// Reconcile unconfirmed sends against their recorded session append
    /// windows, emitting a `chat-send-status` update for each confirmation.
    pub(crate) fn reconcile_chat_send_evidence(&self, tab_id: &str) {
        scan_transcript_evidence(
            &self.agent_registry,
            &self.chat_send_ledger,
            tab_id,
            |status| {
                log::info!(
                    "[chat-send] send_id={} tab={} confirmed by persisted session turn evidence",
                    status.send_id,
                    status.tab_id
                );
                crate::api::events::emit_everywhere(&self.app_handle, "chat-send-status", &status);
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::chat_types::{MessageBlock, RichMessage, SessionDelta};
    use crate::agent::provider::AgentProvider;
    use crate::service::chat_send_transaction::ChatSendState;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    /// Honest byte-window JSONL provider: parses `{"role":"user","content":..}`
    /// lines strictly from `from_offset`, so the boundary semantics under test
    /// are the real append-window semantics, not a mock's.
    struct WindowedJsonlProvider {
        root: PathBuf,
        parse_offsets: Arc<Mutex<Vec<u64>>>,
    }

    impl AgentProvider for WindowedJsonlProvider {
        fn id(&self) -> &str {
            "windowed"
        }
        fn display_name(&self) -> &str {
            "Windowed JSONL"
        }
        fn build_launch_command(&self, _: &crate::agent::provider::AgentLaunchParams) -> String {
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
        fn session_jsonl_path(&self, _cwd: &str, session_id: &str) -> Option<PathBuf> {
            Some(self.root.join(format!("{session_id}.jsonl")))
        }
        fn parse_session_delta(&self, path: &std::path::Path, from_offset: u64) -> SessionDelta {
            self.parse_offsets.lock().unwrap().push(from_offset);
            use std::io::{Read, Seek, SeekFrom};
            let mut file = std::fs::File::open(path).unwrap();
            file.seek(SeekFrom::Start(from_offset)).unwrap();
            let mut window = Vec::new();
            file.read_to_end(&mut window).unwrap();
            let messages = window
                .split(|byte| *byte == b'\n')
                .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
                .filter_map(|value| {
                    let role = value.get("role")?.as_str()?.to_string();
                    let content = value.get("content")?.as_str()?.to_string();
                    Some(RichMessage {
                        role,
                        blocks: vec![MessageBlock {
                            kind: "text".into(),
                            data: serde_json::json!({ "text": content }),
                        }],
                        ..Default::default()
                    })
            })
            .collect();
            SessionDelta {
                new_offset: from_offset + window.len() as u64,
                messages,
            }
        }
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        registry: AgentRegistry,
        ledger: ChatSendLedger,
        parse_offsets: Arc<Mutex<Vec<u64>>>,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().to_path_buf();
            let registry = AgentRegistry::new();
            let parse_offsets = Arc::new(Mutex::new(Vec::new()));
            registry.register_provider(Box::new(WindowedJsonlProvider {
                root: root.clone(),
                parse_offsets: Arc::clone(&parse_offsets),
            }));
            Self {
                _tmp: tmp,
                root,
                registry,
                ledger: ChatSendLedger::default(),
                parse_offsets,
            }
        }

        fn session_path(&self, session_id: &str) -> PathBuf {
            self.root.join(format!("{session_id}.jsonl"))
        }

        fn append_user_turn(&self, session_id: &str, content: &str) {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.session_path(session_id))
                .unwrap();
            writeln!(
                file,
                "{}",
                serde_json::json!({ "role": "user", "content": content })
            )
            .unwrap();
        }

        fn anchor_now(&self, session_id: &str) -> SendEvidenceAnchor {
            let transcript_offset = std::fs::metadata(self.session_path(session_id))
                .map_or(0, |metadata| metadata.len());
            SendEvidenceAnchor {
                provider_id: "windowed".into(),
                cwd: "/proj".into(),
                session_id: session_id.into(),
                window: EvidenceWindow::Jsonl { transcript_offset },
            }
        }

        fn scan(&self, tab_id: &str) -> Vec<ChatSendStatus> {
            let mut confirmed = Vec::new();
            scan_transcript_evidence(&self.registry, &self.ledger, tab_id, |status| {
                confirmed.push(status)
            });
            confirmed
        }
    }

    fn fail_send(ledger: &ChatSendLedger, status: &ChatSendStatus) {
        ledger
            .finish(
                &status.send_id,
                Err("pty_unavailable: false failure".into()),
            )
            .unwrap();
    }

    #[test]
    fn missing_jsonl_store_has_no_unbounded_evidence_window() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("not-materialized.jsonl");

        assert_eq!(jsonl_boundary(Some(path.clone())), None);

        std::fs::write(&path, b"existing-history").unwrap();
        assert_eq!(
            jsonl_boundary(Some(path)),
            Some(EvidenceWindow::Jsonl {
                transcript_offset: 16
            })
        );
    }

    #[test]
    fn chat_closed_delivery_reconciles_from_persisted_turn_on_read() {
        let fx = Fixture::new();
        let send = fx
            .ledger
            .try_start("tab-1", "deploy the fix", Some(fx.anchor_now("sess-a")))
            .unwrap();
        fail_send(&fx.ledger, &send);
        // Chat was closed: no live sink ran. The turn persisted to the session.
        fx.append_user_turn("sess-a", "deploy the fix");
        // RED shape without the scan: the false failure would persist forever.
        assert_eq!(fx.ledger.reconcile("tab-1")[0].state, ChatSendState::Failed);

        let confirmed = fx.scan("tab-1");
        assert_eq!(confirmed.len(), 1);
        assert_eq!(confirmed[0].send_id, send.send_id);
        assert_eq!(confirmed[0].state, ChatSendState::Sent);
        assert_eq!(fx.ledger.reconcile("tab-1")[0].state, ChatSendState::Sent);
    }

    #[test]
    fn exact_turn_lookup_requires_the_anchored_session_append_window() {
        let fx = Fixture::new();
        fx.append_user_turn("sess-a", "prior session A turn");
        fx.append_user_turn("sess-other", "prior session B turn");
        let anchor = fx.anchor_now("sess-a");
        let other_anchor = fx.anchor_now("sess-other");
        fx.append_user_turn("sess-other", "payload");

        assert!(!contains_exact_user_turn(
            &fx.registry,
            &anchor,
            "payload"
        ));
        assert!(contains_exact_user_turn(
            &fx.registry,
            &other_anchor,
            "payload"
        ));
    }

    #[test]
    fn submit_turn_cursor_reads_only_appended_jsonl_since_last_wake() {
        let fx = Fixture::new();
        let anchor = fx.anchor_now("sess-a");
        let mut cursor = ExactTurnCursor::for_anchor(Some(&anchor));
        fx.append_user_turn("sess-a", "unrelated turn");

        assert!(!contains_exact_user_turn_since(
            &fx.registry,
            &anchor,
            "payload",
            &mut cursor,
        ));
        let expected_offset = std::fs::metadata(fx.session_path("sess-a"))
            .unwrap()
            .len();
        fx.append_user_turn("sess-a", "payload");

        assert!(contains_exact_user_turn_since(
            &fx.registry,
            &anchor,
            "payload",
            &mut cursor,
        ));
        assert_eq!(fx.parse_offsets.lock().unwrap()[1], expected_offset);
        assert!(contains_exact_user_turn_since(
            &fx.registry,
            &anchor,
            "payload",
            &mut cursor,
        ));
        assert_eq!(fx.parse_offsets.lock().unwrap().len(), 2);
    }

    #[test]
    fn identical_turn_before_send_boundary_never_confirms() {
        let fx = Fixture::new();
        fx.append_user_turn("sess-a", "deploy the fix");
        let send = fx
            .ledger
            .try_start("tab-1", "deploy the fix", Some(fx.anchor_now("sess-a")))
            .unwrap();
        fail_send(&fx.ledger, &send);

        assert!(fx.scan("tab-1").is_empty());
        let retained = fx.ledger.reconcile("tab-1");
        assert_eq!(retained[0].state, ChatSendState::Failed);
        assert!(retained[0].error.is_some(), "real loss stays actionable");
    }

    #[test]
    fn same_payload_in_another_session_never_confirms() {
        let fx = Fixture::new();
        let send = fx
            .ledger
            .try_start("tab-1", "deploy the fix", Some(fx.anchor_now("sess-a")))
            .unwrap();
        fail_send(&fx.ledger, &send);
        fx.append_user_turn("sess-other", "deploy the fix");

        assert!(fx.scan("tab-1").is_empty());
        assert_eq!(fx.ledger.reconcile("tab-1")[0].state, ChatSendState::Failed);
    }

    #[test]
    fn scan_is_idempotent_and_one_turn_confirms_at_most_one_send() {
        let fx = Fixture::new();
        let first = fx
            .ledger
            .try_start("tab-1", "same text", Some(fx.anchor_now("sess-a")))
            .unwrap();
        let second = fx
            .ledger
            .try_start("tab-1", "same text", Some(fx.anchor_now("sess-a")))
            .unwrap();
        fail_send(&fx.ledger, &first);
        fail_send(&fx.ledger, &second);
        fx.append_user_turn("sess-a", "same text");

        let confirmed = fx.scan("tab-1");
        assert_eq!(confirmed.len(), 1, "one turn confirms exactly one send");
        assert_eq!(confirmed[0].send_id, first.send_id, "oldest first");
        assert!(fx.scan("tab-1").is_empty(), "re-scan consumes nothing new");

        fx.append_user_turn("sess-a", "same text");
        let confirmed = fx.scan("tab-1");
        assert_eq!(confirmed.len(), 1);
        assert_eq!(confirmed[0].send_id, second.send_id);
    }

    #[test]
    fn live_confirmation_counts_as_consumed_evidence_for_the_scan() {
        let fx = Fixture::new();
        let first = fx
            .ledger
            .try_start("tab-1", "same text", Some(fx.anchor_now("sess-a")))
            .unwrap();
        let second = fx
            .ledger
            .try_start("tab-1", "same text", Some(fx.anchor_now("sess-a")))
            .unwrap();
        fx.append_user_turn("sess-a", "same text");
        // The live delta sink already consumed the turn for the oldest send.
        let live = fx
            .ledger
            .confirm_delivered_user_turn("tab-1", "sess-a", "same text")
            .unwrap();
        assert_eq!(live.send_id, first.send_id);
        fail_send(&fx.ledger, &second);

        assert!(
            fx.scan("tab-1").is_empty(),
            "the single persisted turn was already claimed by the live path"
        );
        assert_eq!(fx.ledger.reconcile("tab-1")[0].send_id, second.send_id);
        assert_eq!(fx.ledger.reconcile("tab-1")[0].state, ChatSendState::Failed);
    }

    #[test]
    fn scan_confirmation_wins_over_a_late_transport_failure() {
        let fx = Fixture::new();
        let send = fx
            .ledger
            .try_start("tab-1", "restart dev", Some(fx.anchor_now("sess-a")))
            .unwrap();
        fx.append_user_turn("sess-a", "restart dev");

        let confirmed = fx.scan("tab-1");
        assert_eq!(confirmed.len(), 1);

        let finished = fx
            .ledger
            .finish(
                &send.send_id,
                Err("pty_unavailable: late classification".into()),
            )
            .unwrap();
        assert_eq!(finished.state, ChatSendState::Sent);
        assert_eq!(finished.error, None);
    }

    #[test]
    fn sends_without_an_anchor_are_never_scanned() {
        let fx = Fixture::new();
        let send = fx
            .ledger
            .try_start("tab-1", "deploy the fix", None)
            .unwrap();
        fail_send(&fx.ledger, &send);
        fx.append_user_turn("sess-a", "deploy the fix");

        assert!(fx.scan("tab-1").is_empty());
    }

    #[test]
    fn missing_store_anchor_is_live_only_and_keeps_session_identity() {
        let fx = Fixture::new();
        let send = fx
            .ledger
            .try_start_scoped(
                "tab-1",
                "deploy the fix",
                None,
                Some(SendLiveBinding {
                    session_id: Some("sess-a".into()),
                    binding_nonce: Some("binding-a".into()),
                }),
            )
            .unwrap();
        fail_send(&fx.ledger, &send);
        fx.append_user_turn("sess-a", "deploy the fix");

        assert!(fx.scan("tab-1").is_empty());
        assert!(fx
            .ledger
            .confirm_delivered_user_turn_scoped(
                "tab-1",
                "sess-other",
                Some("binding-a"),
                "deploy the fix",
            )
            .is_none());
        let confirmed = fx
            .ledger
            .confirm_delivered_user_turn_scoped(
                "tab-1",
                "sess-a",
                Some("binding-a"),
                "deploy the fix",
            )
            .unwrap();
        assert_eq!(confirmed.state, ChatSendState::Sent);
    }

    /// Honest cursor-window structured store (the `uses_structured_chat`
    /// shape, e.g. opencode's sqlite): `read_structured_chat` returns rows
    /// strictly after the numeric cursor and the next cursor. No provider-id
    /// branches anywhere — the scan picks this path purely from the anchor's
    /// capability-typed window.
    #[derive(Clone)]
    struct StructuredStoreProvider {
        rows: std::sync::Arc<
            parking_lot::Mutex<
                std::collections::HashMap<String, Vec<crate::agent::chat_types::ChatMsg>>,
            >,
        >,
    }

    impl StructuredStoreProvider {
        fn new() -> Self {
            Self {
                rows: std::sync::Arc::default(),
            }
        }
    }

    impl AgentProvider for StructuredStoreProvider {
        fn id(&self) -> &str {
            "structured-store"
        }
        fn display_name(&self) -> &str {
            "Structured Store"
        }
        fn build_launch_command(&self, _: &crate::agent::provider::AgentLaunchParams) -> String {
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
        fn uses_structured_chat(&self) -> bool {
            true
        }
        fn read_structured_chat(
            &self,
            _cwd: &str,
            session_id: &str,
            cursor: Option<&str>,
        ) -> crate::agent::chat_types::StructuredChat {
            let rows = self.rows.lock();
            let all = rows.get(session_id).cloned().unwrap_or_default();
            let from = cursor.and_then(|c| c.parse::<usize>().ok()).unwrap_or(0);
            crate::agent::chat_types::StructuredChat {
                cursor: Some(all.len().to_string()),
                messages: all.into_iter().skip(from).collect(),
                done: None,
            }
        }
    }

    struct StructuredFixture {
        provider: StructuredStoreProvider,
        registry: AgentRegistry,
        ledger: ChatSendLedger,
    }

    impl StructuredFixture {
        fn new() -> Self {
            let provider = StructuredStoreProvider::new();
            let registry = AgentRegistry::new();
            registry.register_provider(Box::new(provider.clone()));
            Self {
                provider,
                registry,
                ledger: ChatSendLedger::default(),
            }
        }

        fn append_user_row(&self, session_id: &str, content: &str) {
            self.provider
                .rows
                .lock()
                .entry(session_id.to_string())
                .or_default()
                .push(crate::agent::chat_types::ChatMsg {
                    id: uuid::Uuid::new_v4().to_string(),
                    kind: "user".into(),
                    content: content.into(),
                    ..Default::default()
                });
        }

        fn anchor_now(&self, session_id: &str) -> SendEvidenceAnchor {
            let chat = self
                .provider
                .read_structured_chat("/proj", session_id, None);
            SendEvidenceAnchor {
                provider_id: "structured-store".into(),
                cwd: "/proj".into(),
                session_id: session_id.into(),
                window: structured_boundary(&chat).expect("cursor-bearing store"),
            }
        }

        fn scan(&self, tab_id: &str) -> Vec<ChatSendStatus> {
            let mut confirmed = Vec::new();
            scan_transcript_evidence(&self.registry, &self.ledger, tab_id, |status| {
                confirmed.push(status)
            });
            confirmed
        }
    }

    #[test]
    fn structured_store_detached_delivery_reconciles_on_read() {
        let fx = StructuredFixture::new();
        fx.append_user_row("sess-s", "earlier unrelated turn");
        let send = fx
            .ledger
            .try_start("tab-1", "deploy the fix", Some(fx.anchor_now("sess-s")))
            .unwrap();
        fail_send(&fx.ledger, &send);
        // Chat detached before evidence: no poller subscription survived. The
        // exact turn still persisted to the structured store.
        fx.append_user_row("sess-s", "deploy the fix");

        let confirmed = fx.scan("tab-1");
        assert_eq!(confirmed.len(), 1);
        assert_eq!(confirmed[0].send_id, send.send_id);
        assert_eq!(fx.ledger.reconcile("tab-1")[0].state, ChatSendState::Sent);
    }

    #[test]
    fn structured_pre_send_and_foreign_session_rows_never_confirm() {
        let fx = StructuredFixture::new();
        fx.append_user_row("sess-s", "deploy the fix");
        let send = fx
            .ledger
            .try_start("tab-1", "deploy the fix", Some(fx.anchor_now("sess-s")))
            .unwrap();
        fail_send(&fx.ledger, &send);
        fx.append_user_row("sess-other", "deploy the fix");

        assert!(
            fx.scan("tab-1").is_empty(),
            "pre-send + foreign rows are not evidence"
        );
        let retained = fx.ledger.reconcile("tab-1");
        assert_eq!(retained[0].state, ChatSendState::Failed);
        assert!(
            retained[0].error.is_some(),
            "absent evidence stays actionable"
        );
    }

    #[test]
    fn structured_one_turn_confirms_one_send_and_rescan_is_idempotent() {
        let fx = StructuredFixture::new();
        let first = fx
            .ledger
            .try_start("tab-1", "same text", Some(fx.anchor_now("sess-s")))
            .unwrap();
        let second = fx
            .ledger
            .try_start("tab-1", "same text", Some(fx.anchor_now("sess-s")))
            .unwrap();
        fail_send(&fx.ledger, &first);
        fail_send(&fx.ledger, &second);
        fx.append_user_row("sess-s", "same text");

        let confirmed = fx.scan("tab-1");
        assert_eq!(confirmed.len(), 1);
        assert_eq!(confirmed[0].send_id, first.send_id, "oldest first");
        assert!(fx.scan("tab-1").is_empty(), "re-scan consumes nothing new");

        fx.append_user_row("sess-s", "same text");
        let confirmed = fx.scan("tab-1");
        assert_eq!(confirmed.len(), 1);
        assert_eq!(confirmed[0].send_id, second.send_id);
    }

    #[test]
    fn cursorless_nonempty_structured_store_mints_no_boundary() {
        let populated = crate::agent::chat_types::StructuredChat {
            cursor: None,
            messages: vec![crate::agent::chat_types::ChatMsg::default()],
            done: None,
        };
        assert_eq!(structured_boundary(&populated), None);

        let empty = crate::agent::chat_types::StructuredChat::default();
        assert_eq!(
            structured_boundary(&empty),
            Some(EvidenceWindow::Structured { cursor: None }),
            "an empty store is a valid boundary: every future row is post-send"
        );
    }
}
