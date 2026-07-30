//! Closed-chat evidence reconciliation for Chat sends.
//!
//! A send whose live delta stream was not running (Chat closed during
//! delivery) can still be classified honestly: each send records an
//! authoritative [`SendEvidenceAnchor`] (provider, cwd, session id, transcript
//! byte offset at send start), and on ledger read the scan inspects ONLY that
//! session's append window at/after the boundary. Pre-send history, other
//! sessions, and already-consumed turns can never confirm a send.

use crate::agent::registry::AgentRegistry;
use crate::service::chat_send_transaction::{
    normalize_turn_evidence, ChatSendLedger, ChatSendStatus, SendEvidenceAnchor,
};
use crate::service::ServiceContext;

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
        let Some(path) = registry
            .with_provider(&anchor.provider_id, |provider| {
                provider.session_jsonl_path(&anchor.cwd, &anchor.session_id)
            })
            .flatten()
        else {
            continue;
        };
        if !path.exists() {
            continue;
        }
        let Some(delta) = registry.with_provider(&anchor.provider_id, |provider| {
            provider.parse_session_delta(&path, anchor.transcript_offset)
        }) else {
            continue;
        };
        let matching_turns = delta
            .messages
            .iter()
            .enumerate()
            .flat_map(|(index, rich)| crate::service::tab_chat::rich_to_chat_msgs(rich, index))
            .filter(|msg| msg.kind == "user")
            .filter(|msg| normalize_turn_evidence(&msg.content) == candidate.payload)
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
    /// Capture the send's authoritative transcript identity and append-window
    /// boundary at send start. `None` when the tab has no bound agent session
    /// yet — such a send can only be confirmed by the live delta stream.
    pub(crate) fn chat_send_anchor(&self, tab_id: &str) -> Option<SendEvidenceAnchor> {
        let (provider_id, cwd, session_id) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id)?;
            (
                tab.agent_type.clone()?,
                tab.cwd.clone().unwrap_or_default(),
                tab.agent_session_id.clone()?,
            )
        };
        let transcript_offset = self
            .agent_registry
            .with_provider(&provider_id, |provider| {
                provider.session_jsonl_path(&cwd, &session_id)
            })
            .flatten()
            .and_then(|path| std::fs::metadata(path).ok())
            .map_or(0, |metadata| metadata.len());
        Some(SendEvidenceAnchor {
            provider_id,
            cwd,
            session_id,
            transcript_offset,
        })
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
    use std::sync::Arc;

    /// Honest byte-window JSONL provider: parses `{"role":"user","content":..}`
    /// lines strictly from `from_offset`, so the boundary semantics under test
    /// are the real append-window semantics, not a mock's.
    struct WindowedJsonlProvider {
        root: PathBuf,
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
            let bytes = std::fs::read(path).unwrap_or_default();
            let window = &bytes[(from_offset as usize).min(bytes.len())..];
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
                new_offset: bytes.len() as u64,
                messages,
            }
        }
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        registry: AgentRegistry,
        ledger: ChatSendLedger,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let root = tmp.path().to_path_buf();
            let registry = AgentRegistry::new();
            registry.register_provider(Box::new(WindowedJsonlProvider { root: root.clone() }));
            Self {
                _tmp: tmp,
                root,
                registry,
                ledger: ChatSendLedger::default(),
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
                transcript_offset,
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
            .finish(&status.send_id, Err("pty_unavailable: false failure".into()))
            .unwrap();
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
            .confirm_delivered_user_turn("tab-1", "same text")
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
            .finish(&send.send_id, Err("pty_unavailable: late classification".into()))
            .unwrap();
        assert_eq!(finished.state, ChatSendState::Sent);
        assert_eq!(finished.error, None);
    }

    #[test]
    fn sends_without_an_anchor_are_never_scanned() {
        let fx = Fixture::new();
        let send = fx.ledger.try_start("tab-1", "deploy the fix", None).unwrap();
        fail_send(&fx.ledger, &send);
        fx.append_user_turn("sess-a", "deploy the fix");

        assert!(fx.scan("tab-1").is_empty());
    }
}
