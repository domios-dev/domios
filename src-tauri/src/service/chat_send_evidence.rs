//! Closed-chat evidence reconciliation for Chat sends.
//!
//! A send whose live delta stream was not running (Chat closed during
//! delivery) can still be classified honestly: each send captures its bound
//! session and a safe [`SendEvidenceAnchor`] when one exists. Ledger reads
//! inspect ONLY that session's bounded append window. Pre-send history, other
//! sessions, and already-used turns can never confirm a send.

use crate::agent::registry::AgentRegistry;
use crate::service::chat_send_transaction::{
    normalize_turn_evidence, ChatSendLedger, ChatSendStatus, EvidenceCandidate, EvidenceWindow,
    SendEvidenceAnchor, SendLiveBinding,
};
use crate::service::ServiceContext;

#[derive(Clone, Debug, PartialEq, Eq)]
struct BoundEvidenceSource {
    provider_id: String,
    cwd: String,
    session_id: String,
    binding_nonce: Option<String>,
}

fn window_user_turns(
    registry: &AgentRegistry,
    anchor: &SendEvidenceAnchor,
    session_id: &str,
) -> Vec<String> {
    match &anchor.window {
        EvidenceWindow::Jsonl { transcript_offset } => {
            jsonl_window_user_turns(registry, anchor, session_id, *transcript_offset)
        }
        EvidenceWindow::JsonlMaterialization => {
            jsonl_window_user_turns(registry, anchor, session_id, 0)
        }
        EvidenceWindow::JsonlSessionMaterialization { not_before_ms } => {
            jsonl_window_user_turns_after(registry, anchor, session_id, 0, Some(*not_before_ms))
        }
        EvidenceWindow::Structured { cursor } => registry
            .with_provider(&anchor.provider_id, |provider| {
                provider.read_structured_chat(&anchor.cwd, session_id, cursor.as_deref())
            })
            .map(|chat| {
                chat.messages
                    .into_iter()
                    .filter(|msg| msg.kind == "user")
                    .map(|msg| msg.content)
                    .collect()
            })
            .unwrap_or_default(),
        EvidenceWindow::StructuredMaterialization { not_before_ms } => {
            structured_materialization_user_turns(registry, anchor, session_id, *not_before_ms)
        }
    }
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

/// One turn instance confirms at most one send. Nested append windows remain
/// jointly attributable only when every newer suffix has enough exact turns.
fn scan_transcript_evidence(
    registry: &AgentRegistry,
    ledger: &ChatSendLedger,
    tab_id: &str,
    source: &BoundEvidenceSource,
    mut on_confirmed: impl FnMut(ChatSendStatus),
) {
    for mut candidate in ledger.evidence_candidates(tab_id) {
        if !candidate_matches_source(&candidate, source) {
            continue;
        }
        let session_id = match candidate.anchor.session_id.clone() {
            Some(session_id) => session_id,
            None if jsonl_session_materialized_for_current_binding(&candidate, source) => {
                if !ledger.bind_evidence_session(&candidate.send_id, &source.session_id) {
                    continue;
                }
                candidate.anchor.session_id = Some(source.session_id.clone());
                source.session_id.clone()
            }
            None if structured_session_matches_current_binding(&candidate, source) => {
                if !ledger.bind_evidence_session(&candidate.send_id, &source.session_id) {
                    continue;
                }
                candidate.anchor.session_id = Some(source.session_id.clone());
                source.session_id.clone()
            }
            None => continue,
        };
        if candidate_has_attributable_turn(
            registry,
            ledger,
            tab_id,
            source,
            &candidate,
            &session_id,
        ) {
            if let Some(status) = ledger.confirm_send(&candidate.send_id) {
                on_confirmed(status);
            }
        }
    }
}

fn candidate_has_attributable_turn(
    registry: &AgentRegistry,
    ledger: &ChatSendLedger,
    tab_id: &str,
    source: &BoundEvidenceSource,
    candidate: &EvidenceCandidate,
    session_id: &str,
) -> bool {
    let mut claims = ledger.confirmed_evidence_claims(tab_id, session_id, &candidate.payload);
    claims.push(candidate.clone());
    let matching_counts = claims
        .iter()
        .map(|claim| {
            if claim.anchor.provider_id != source.provider_id
                || claim.anchor.cwd != source.cwd
                || claim.anchor.session_id.as_deref() != Some(session_id)
            {
                return None;
            }
            Some(
                window_user_turns(registry, &claim.anchor, session_id)
                    .iter()
                    .filter(|content| normalize_turn_evidence(content) == candidate.payload)
                    .count(),
            )
        })
        .collect::<Option<Vec<_>>>();
    let Some(mut matching_counts) = matching_counts else {
        return false;
    };
    matching_counts.sort_unstable();
    matching_counts
        .iter()
        .enumerate()
        .all(|(index, count)| *count >= index + 1)
}

fn jsonl_window_user_turns(
    registry: &AgentRegistry,
    anchor: &SendEvidenceAnchor,
    session_id: &str,
    transcript_offset: u64,
) -> Vec<String> {
    jsonl_window_user_turns_after(registry, anchor, session_id, transcript_offset, None)
}

fn jsonl_window_user_turns_after(
    registry: &AgentRegistry,
    anchor: &SendEvidenceAnchor,
    session_id: &str,
    transcript_offset: u64,
    not_before_ms: Option<u64>,
) -> Vec<String> {
    let Some(path) = registry
        .with_provider(&anchor.provider_id, |provider| {
            provider.session_jsonl_path(&anchor.cwd, session_id)
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
            provider.parse_session_delta(&path, transcript_offset)
        })
        .map(|delta| {
            delta
                .messages
                .iter()
                .filter(|rich| {
                    not_before_ms.is_none_or(|boundary| {
                        rich.timestamp
                            .as_deref()
                            .and_then(|timestamp| {
                                chrono::DateTime::parse_from_rfc3339(timestamp).ok()
                            })
                            .and_then(|timestamp| u64::try_from(timestamp.timestamp_millis()).ok())
                            .is_some_and(|timestamp| timestamp > boundary)
                    })
                })
                .enumerate()
                .flat_map(|(index, rich)| crate::service::tab_chat::rich_to_chat_msgs(rich, index))
                .filter(|msg| msg.kind == "user")
                .map(|msg| msg.content)
                .collect()
        })
        .unwrap_or_default()
}

fn candidate_matches_source(candidate: &EvidenceCandidate, source: &BoundEvidenceSource) -> bool {
    let anchor = &candidate.anchor;
    if anchor.provider_id != source.provider_id
        || anchor.cwd != source.cwd
        || anchor
            .session_id
            .as_deref()
            .is_some_and(|session_id| session_id != source.session_id)
    {
        return false;
    }
    candidate.live_binding.as_ref().is_none_or(|binding| {
        binding
            .session_id
            .as_deref()
            .is_none_or(|session_id| session_id == source.session_id)
            && binding.binding_nonce.as_deref() == source.binding_nonce.as_deref()
    })
}

fn jsonl_session_materialized_for_current_binding(
    candidate: &EvidenceCandidate,
    source: &BoundEvidenceSource,
) -> bool {
    matches!(
        candidate.anchor.window,
        EvidenceWindow::JsonlSessionMaterialization { .. }
    ) && unbound_launch_matches_current_binding(candidate, source)
}

fn structured_session_matches_current_binding(
    candidate: &EvidenceCandidate,
    source: &BoundEvidenceSource,
) -> bool {
    matches!(
        candidate.anchor.window,
        EvidenceWindow::StructuredMaterialization { .. }
    ) && unbound_launch_matches_current_binding(candidate, source)
}

fn unbound_launch_matches_current_binding(
    candidate: &EvidenceCandidate,
    source: &BoundEvidenceSource,
) -> bool {
    candidate.live_binding.as_ref().is_some_and(|binding| {
        binding.session_id.is_none()
            && binding.binding_nonce.is_some()
            && binding.binding_nonce == source.binding_nonce
    })
}

fn structured_materialization_user_turns(
    registry: &AgentRegistry,
    anchor: &SendEvidenceAnchor,
    session_id: &str,
    not_before_ms: u64,
) -> Vec<String> {
    let created_after_boundary = registry
        .with_provider(&anchor.provider_id, |provider| {
            provider.session_created_at(&anchor.cwd, session_id)
        })
        .flatten()
        .and_then(|created_at| created_at.duration_since(std::time::UNIX_EPOCH).ok())
        .is_some_and(|created_at| created_at.as_millis() > u128::from(not_before_ms));
    registry
        .with_provider(&anchor.provider_id, |provider| {
            provider.read_structured_chat(&anchor.cwd, session_id, None)
        })
        .map(|chat| {
            chat.messages
                .into_iter()
                .filter(|msg| {
                    msg.kind == "user"
                        && (created_after_boundary
                            || msg
                                .timestamp
                                .as_deref()
                                .and_then(source_timestamp_ms)
                                .is_some_and(|timestamp| timestamp > not_before_ms))
                })
                .map(|msg| msg.content)
                .collect()
        })
        .unwrap_or_default()
}

fn source_timestamp_ms(timestamp: &str) -> Option<u64> {
    timestamp.parse::<u64>().ok().or_else(|| {
        chrono::DateTime::parse_from_rfc3339(timestamp)
            .ok()
            .and_then(|timestamp| u64::try_from(timestamp.timestamp_millis()).ok())
    })
}

impl ServiceContext {
    /// Capture the current launch's session plus its safe append-window anchor.
    /// Stores without an attributable boundary stay live-sink-only.
    pub(crate) fn chat_send_anchor(&self, tab_id: &str) -> CapturedSendEvidence {
        let captured_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as u64)
            .unwrap_or(0);
        let captured = {
            let mux = self.mux.read();
            mux.get_tab(tab_id).map(|tab| {
                (
                    tab.agent_type.clone(),
                    tab.cwd.clone().unwrap_or_default(),
                    tab.agent_session_id.clone(),
                    tab.session_binding_nonce.clone(),
                    tab.reported_session_binding_nonce.clone(),
                    tab.agent_set_at.map(|set_at| set_at.elapsed()),
                    mux.other_session_ids(tab_id),
                )
            })
        };
        let Some((
            provider_id,
            cwd,
            session_id,
            binding_nonce,
            reported_binding_nonce,
            set_at_elapsed,
            excluded,
        )) = captured
        else {
            return CapturedSendEvidence::default();
        };
        let session_was_bound = session_id.is_some();
        let live_binding =
            (session_id.is_some() || binding_nonce.is_some()).then(|| SendLiveBinding {
                session_id: session_id.clone(),
                binding_nonce: binding_nonce.clone(),
            });
        let Some(provider_id) = provider_id else {
            return CapturedSendEvidence {
                anchor: None,
                live_binding,
            };
        };
        let (structured, can_detect_unbound, has_hooks) = self
            .agent_registry
            .with_provider(&provider_id, |provider| {
                (
                    provider.uses_structured_chat(),
                    matches!(
                        provider.session_id_injection(),
                        crate::agent::provider::SessionIdInjection::None
                    ),
                    provider.has_hooks(),
                )
            })
            .unwrap_or((false, false, false));
        let session_id = session_id.or_else(|| {
            if structured {
                return None;
            }
            let max_age = set_at_elapsed? + std::time::Duration::from_secs(30);
            binding_nonce.as_ref()?;
            if !can_detect_unbound {
                return None;
            }
            self.agent_registry
                .with_provider(&provider_id, |provider| {
                    provider.detect_session_id(&cwd, &excluded, Some(max_age))
                })
                .flatten()
        });
        let anchor = match session_id {
            Some(session_id) => {
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
                    jsonl_boundary(path.clone()).or_else(|| {
                        (session_was_bound
                            && binding_nonce.is_some()
                            && binding_nonce == reported_binding_nonce
                            && path.is_some_and(|path| !path.exists()))
                        .then_some(EvidenceWindow::JsonlMaterialization)
                    })
                };
                window.map(|window| SendEvidenceAnchor {
                    provider_id,
                    cwd,
                    session_id: Some(session_id),
                    window,
                })
            }
            None if structured
                && can_detect_unbound
                && binding_nonce.is_some()
                && set_at_elapsed.is_some() =>
            {
                Some(SendEvidenceAnchor {
                    provider_id,
                    cwd,
                    session_id: None,
                    window: EvidenceWindow::StructuredMaterialization {
                        not_before_ms: captured_at_ms,
                    },
                })
            }
            None if !structured
                && can_detect_unbound
                && has_hooks
                && binding_nonce.is_some()
                && set_at_elapsed.is_some() =>
            {
                Some(SendEvidenceAnchor {
                    provider_id,
                    cwd,
                    session_id: None,
                    window: EvidenceWindow::JsonlSessionMaterialization {
                        not_before_ms: captured_at_ms,
                    },
                })
            }
            None => None,
        };
        CapturedSendEvidence {
            anchor,
            live_binding,
        }
    }

    /// Reconcile unconfirmed sends against their recorded session append
    /// windows, emitting a `chat-send-status` update for each confirmation.
    pub(crate) fn reconcile_chat_send_evidence(&self, tab_id: &str) {
        let source = {
            let mux = self.mux.read();
            mux.get_tab(tab_id).and_then(|tab| {
                let binding_nonce = tab.session_binding_nonce.clone()?;
                if tab.reported_session_binding_nonce.as_ref() != Some(&binding_nonce) {
                    return None;
                }
                Some(BoundEvidenceSource {
                    provider_id: tab.agent_type.clone()?,
                    cwd: tab.cwd.clone().unwrap_or_default(),
                    session_id: tab.agent_session_id.clone()?,
                    binding_nonce: Some(binding_nonce),
                })
            })
        };
        let Some(source) = source else {
            return;
        };
        scan_transcript_evidence(
            &self.agent_registry,
            &self.chat_send_ledger,
            tab_id,
            &source,
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
            exclude: &[String],
            _: Option<std::time::Duration>,
        ) -> Option<String> {
            std::fs::read_dir(&self.root)
                .ok()?
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    entry
                        .path()
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .map(str::to_string)
                })
                .find(|session_id| !exclude.contains(session_id))
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
                        timestamp: value
                            .get("timestamp")
                            .and_then(|timestamp| timestamp.as_str())
                            .map(str::to_string),
                        ..Default::default()
                    })
                })
                .collect();
            SessionDelta {
                new_offset: bytes.len() as u64,
                messages,
            }
        }
        fn has_hooks(&self) -> bool {
            true
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
                session_id: Some(session_id.into()),
                window: EvidenceWindow::Jsonl { transcript_offset },
            }
        }

        fn scan(&self, tab_id: &str) -> Vec<ChatSendStatus> {
            let mut confirmed = Vec::new();
            let Some(candidate) = self.ledger.evidence_candidates(tab_id).into_iter().next() else {
                return confirmed;
            };
            let source = BoundEvidenceSource {
                provider_id: candidate.anchor.provider_id.clone(),
                cwd: candidate.anchor.cwd.clone(),
                session_id: candidate.anchor.session_id.clone().unwrap(),
                binding_nonce: candidate
                    .live_binding
                    .and_then(|binding| binding.binding_nonce),
            };
            scan_transcript_evidence(&self.registry, &self.ledger, tab_id, &source, |status| {
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
        created_at: std::sync::Arc<
            parking_lot::Mutex<std::collections::HashMap<String, std::time::SystemTime>>,
        >,
    }

    impl StructuredStoreProvider {
        fn new() -> Self {
            Self {
                rows: std::sync::Arc::default(),
                created_at: std::sync::Arc::default(),
            }
        }

        fn append_user_row(&self, session_id: &str, content: &str) {
            self.created_at
                .lock()
                .entry(session_id.to_string())
                .or_insert_with(std::time::SystemTime::now);
            self.rows
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
            exclude: &[String],
            _: Option<std::time::Duration>,
        ) -> Option<String> {
            self.rows
                .lock()
                .keys()
                .find(|session_id| !exclude.contains(session_id))
                .cloned()
        }
        fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
            None
        }
        fn uses_structured_chat(&self) -> bool {
            true
        }
        fn session_created_at(
            &self,
            _cwd: &str,
            session_id: &str,
        ) -> Option<std::time::SystemTime> {
            self.created_at.lock().get(session_id).copied()
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
            self.provider.append_user_row(session_id, content);
        }

        fn anchor_now(&self, session_id: &str) -> SendEvidenceAnchor {
            let chat = self
                .provider
                .read_structured_chat("/proj", session_id, None);
            SendEvidenceAnchor {
                provider_id: "structured-store".into(),
                cwd: "/proj".into(),
                session_id: Some(session_id.into()),
                window: structured_boundary(&chat).expect("cursor-bearing store"),
            }
        }

        fn scan(&self, tab_id: &str) -> Vec<ChatSendStatus> {
            let mut confirmed = Vec::new();
            let Some(candidate) = self.ledger.evidence_candidates(tab_id).into_iter().next() else {
                return confirmed;
            };
            let source = BoundEvidenceSource {
                provider_id: candidate.anchor.provider_id.clone(),
                cwd: candidate.anchor.cwd.clone(),
                session_id: candidate.anchor.session_id.clone().unwrap(),
                binding_nonce: candidate
                    .live_binding
                    .and_then(|binding| binding.binding_nonce),
            };
            scan_transcript_evidence(&self.registry, &self.ledger, tab_id, &source, |status| {
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

    mod status_tests;
}
