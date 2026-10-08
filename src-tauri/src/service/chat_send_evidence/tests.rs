use super::*;
use crate::agent::chat_types::{MessageBlock, RichMessage, SessionDelta};
use crate::agent::provider::AgentProvider;
use crate::service::chat_send_transaction::ChatSendState;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

mod status_tests;
mod structured_sequential_tests;
mod trace_tests;

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
                    uuid: value
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string),
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
        let transcript_offset =
            std::fs::metadata(self.session_path(session_id)).map_or(0, |metadata| metadata.len());
        SendEvidenceAnchor {
            provider_id: "windowed".into(),
            cwd: "/proj".into(),
            session_id: session_id.into(),
            window: EvidenceWindow::Jsonl { transcript_offset },
            turn_normalization: Default::default(),
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
    let source = JsonlSource::default();

    let (anchor, witness) = jsonl_boundary(Some(path.clone()), &source, "sess-a");
    assert!(anchor.is_none());
    assert!(witness.is_some());

    std::fs::write(&path, b"existing-history").unwrap();
    assert_eq!(
        jsonl_boundary(Some(path), &source, "sess-a").0,
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
fn retired_agent_delivery_requires_an_exact_turn_in_its_own_append_window() {
    let fx = Fixture::new();
    fx.append_user_turn("sess-a", "same payload");
    let anchor = fx.anchor_now("sess-a");
    fx.ledger
        .track_retired_agent_delivery("tab-1", "delivery-a", 1, "same payload", anchor);

    fx.append_user_turn("sess-other", "same payload");
    assert!(fx.scan("tab-1").is_empty());
    assert_eq!(
        fx.ledger.status("delivery-a").unwrap().state,
        ChatSendState::Unknown
    );

    fx.append_user_turn("sess-a", "same payload");
    let confirmed = fx.scan("tab-1");
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed[0].send_id, "delivery-a");
    assert_eq!(confirmed[0].state, ChatSendState::Sent);
}

#[test]
fn exact_turn_lookup_requires_the_anchored_session_append_window() {
    let fx = Fixture::new();
    fx.append_user_turn("sess-a", "prior session A turn");
    fx.append_user_turn("sess-other", "prior session B turn");
    let anchor = fx.anchor_now("sess-a");
    let other_anchor = fx.anchor_now("sess-other");
    fx.append_user_turn("sess-other", "payload");

    assert!(!contains_exact_user_turn(&fx.registry, &anchor, "payload"));
    assert!(contains_exact_user_turn(
        &fx.registry,
        &other_anchor,
        "payload"
    ));
}

#[test]
fn provider_normalization_never_matches_a_turn_from_another_session() {
    let fx = Fixture::new();
    fx.append_user_turn("sess-other", "inspect  logs");
    let mut anchor = fx.anchor_now("sess-a");
    anchor.turn_normalization = crate::agent::provider::TurnEvidenceNormalization {
        drop_supplementary_characters: true,
        ..Default::default()
    };

    assert!(!contains_exact_user_turn(
        &fx.registry,
        &anchor,
        "inspect 🚀 logs"
    ));

    fx.append_user_turn("sess-a", "inspect  logs");
    assert!(contains_exact_user_turn(
        &fx.registry,
        &anchor,
        "inspect 🚀 logs"
    ));
}

#[test]
fn exact_turn_matches_typed_route_with_wrapped_body_only() {
    let fx = Fixture::new();
    let mut anchor = fx.anchor_now("sess-a");
    anchor.turn_normalization = crate::agent::provider::TurnEvidenceNormalization {
        unwrap_pasted_content_wrapper: true,
        collapse_horizontal_whitespace: true,
        drop_supplementary_characters: false,
        drop_line_breaks: false,
    };
    let open = "<domios from=\"tab\" label=\"sender\" tab=\"abc12345\">";
    let body = "opaque routed body";
    let suffix = format!(
        " [reply] {}{}",
        codeterm_types::domios::DOMIOS_TYPED_INSTRUCTION,
        codeterm_types::domios::DOMIOS_CLOSE_TAG
    );
    let expected = format!("{open}{body}{suffix}");
    let observed = format!(
        "{open}<pasted_content id=\"evidence\">\n{body}\n</pasted_content id=\"evidence\">{suffix}"
    );
    fx.append_user_turn("sess-a", &observed);

    assert!(contains_exact_user_turn(&fx.registry, &anchor, &expected));
    assert!(contains_exact_user_turn_since(
        &fx.registry,
        &anchor,
        &expected,
        &mut ExactTurnCursor::for_anchor(Some(&anchor)),
    ));
    assert_eq!(
        crate::service::chat_send_transaction::normalize_turn_evidence_with(
            &observed,
            anchor.turn_normalization,
        ),
        crate::service::chat_send_transaction::normalize_turn_evidence_with(
            &expected,
            anchor.turn_normalization,
        )
    );
}

#[test]
fn exact_turn_evidence_matches_a_12k_user_paste() {
    let fx = Fixture::new();
    let body = format!("paste-start:{}:paste-end", "x".repeat(12 * 1024));
    let mut anchor = fx.anchor_now("sess-a");
    anchor.turn_normalization = crate::agent::provider::TurnEvidenceNormalization {
        unwrap_pasted_content_wrapper: true,
        collapse_horizontal_whitespace: true,
        drop_supplementary_characters: false,
        drop_line_breaks: false,
    };
    let open = "<domios from=\"tab\" label=\"sender\" tab=\"abc12345\">";
    let suffix = format!(
        " [reply] {}{}",
        codeterm_types::domios::DOMIOS_TYPED_INSTRUCTION,
        codeterm_types::domios::DOMIOS_CLOSE_TAG
    );
    let expected = format!("{open}{body}{suffix}");
    let observed = format!(
        "{open}<pasted_content id=\"long-paste\">\n{body}\n</pasted_content id=\"long-paste\">{suffix}"
    );
    fx.append_user_turn("sess-a", &observed);

    assert!(body.len() >= 12 * 1024);
    assert!(contains_exact_user_turn(&fx.registry, &anchor, &expected));
    assert!(contains_exact_user_turn_since(
        &fx.registry,
        &anchor,
        &expected,
        &mut ExactTurnCursor::for_anchor(Some(&anchor)),
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
    let expected_offset = std::fs::metadata(fx.session_path("sess-a")).unwrap().len();
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
fn persisted_merged_turn_confirms_three_sends_from_its_append_window() {
    let fx = Fixture::new();
    let anchor = fx.anchor_now("sess-a");
    let sends = ["alpha-opaque", "beta-opaque", "gamma-opaque"]
        .into_iter()
        .map(|body| {
            fx.ledger
                .try_start("tab-1", body, Some(anchor.clone()))
                .unwrap()
        })
        .collect::<Vec<_>>();
    for send in &sends {
        fail_send(&fx.ledger, send);
    }
    fx.append_user_turn("sess-a", "alpha-opaquebeta-opaquegamma-opaque");

    let confirmed = fx.scan("tab-1");

    assert_eq!(confirmed.len(), 3);
    assert_eq!(
        confirmed
            .iter()
            .map(|status| status.send_id.as_str())
            .collect::<Vec<_>>(),
        sends
            .iter()
            .map(|status| status.send_id.as_str())
            .collect::<Vec<_>>(),
    );
    assert!(confirmed
        .iter()
        .all(|status| status.state == ChatSendState::Sent));
    assert!(fx.scan("tab-1").is_empty());
}

#[test]
fn merged_turn_is_not_reused_as_an_exact_echo_for_a_later_send() {
    let fx = Fixture::new();
    let anchor = fx.anchor_now("sess-a");
    let sends = ["alpha", "beta", "alphabeta"]
        .into_iter()
        .map(|body| {
            fx.ledger
                .try_start("tab-1", body, Some(anchor.clone()))
                .unwrap()
        })
        .collect::<Vec<_>>();
    for send in &sends {
        fx.ledger
            .finish(
                &send.send_id,
                Err("submit_unconfirmed: waiting for an exact user turn".into()),
            )
            .unwrap();
    }
    fx.append_user_turn("sess-a", "alphabeta");

    let confirmed = fx.scan("tab-1");

    assert_eq!(confirmed.len(), 2);
    assert_eq!(confirmed[0].send_id, sends[0].send_id);
    assert_eq!(confirmed[1].send_id, sends[1].send_id);
    assert_eq!(
        fx.ledger.status(&sends[2].send_id).unwrap().state,
        ChatSendState::Unknown
    );
    assert!(fx.scan("tab-1").is_empty());
    assert_eq!(
        fx.ledger.status(&sends[2].send_id).unwrap().state,
        ChatSendState::Unknown
    );
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
    let retained = fx.ledger.reconcile("tab-1");
    let state_of = |send_id: &str| {
        retained
            .iter()
            .find(|status| status.send_id == send_id)
            .map(|status| status.state.clone())
    };
    assert_eq!(state_of(&first.send_id), Some(ChatSendState::Sent));
    assert_eq!(state_of(&second.send_id), Some(ChatSendState::Failed));
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
        .confirm_delivered_user_turn_scoped("tab-1", "sess-a", Some("binding-a"), "deploy the fix")
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
            turn_normalization: Default::default(),
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
