use crate::agent::chat_types::{ChatMsg, MessageBlock, RichMessage, SessionDelta, StructuredChat};
use crate::agent::provider::{
    AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery, TurnEvidenceNormalization,
};
use crate::service::chat_send_transaction::ChatSendState;
use crate::service::test_fixture::ServiceFixture;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Copy)]
enum StoreKind {
    Jsonl,
    Structured,
}

#[derive(Clone)]
struct LazyFirstTurnProvider {
    root: PathBuf,
    kind: StoreKind,
    structured_rows: Arc<Mutex<HashMap<String, Vec<ChatMsg>>>>,
}

impl AgentProvider for LazyFirstTurnProvider {
    fn id(&self) -> &str {
        "lazy-first-turn-fixture"
    }

    fn display_name(&self) -> &str {
        "Lazy first turn fixture"
    }

    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }

    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }

    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }

    fn session_starts_on_first_user_turn(&self) -> bool {
        true
    }

    fn turn_evidence_normalization(&self) -> TurnEvidenceNormalization {
        TurnEvidenceNormalization {
            drop_supplementary_characters: true,
            ..Default::default()
        }
    }

    fn detect_from_title(&self, _: &str) -> bool {
        false
    }

    fn detect_from_output(&self, _: &str) -> bool {
        false
    }

    fn session_file_path(&self, _: &str, session_id: &str) -> Option<PathBuf> {
        matches!(self.kind, StoreKind::Jsonl).then(|| self.root.join(format!("{session_id}.jsonl")))
    }

    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }

    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }

    fn session_jsonl_path(&self, _: &str, session_id: &str) -> Option<PathBuf> {
        self.session_file_path("", session_id)
    }

    fn parse_session_delta(&self, path: &Path, from_offset: u64) -> SessionDelta {
        use std::io::{Read, Seek, SeekFrom};

        let Ok(mut file) = std::fs::File::open(path) else {
            return SessionDelta {
                new_offset: from_offset,
                messages: Vec::new(),
            };
        };
        if file.seek(SeekFrom::Start(from_offset)).is_err() {
            return SessionDelta {
                new_offset: from_offset,
                messages: Vec::new(),
            };
        }
        let mut bytes = Vec::new();
        if file.read_to_end(&mut bytes).is_err() {
            return SessionDelta {
                new_offset: from_offset,
                messages: Vec::new(),
            };
        }
        let messages = bytes
            .split(|byte| *byte == b'\n')
            .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
            .filter_map(|value| {
                let (role, text) = match self.kind {
                    StoreKind::Jsonl => {
                        let payload = value.get("payload")?;
                        let content = payload.get("content")?.as_array()?;
                        let text = content
                            .iter()
                            .filter_map(|block| {
                                (block.get("type")?.as_str()? == "input_text")
                                    .then(|| block.get("text")?.as_str())
                                    .flatten()
                            })
                            .collect::<String>();
                        (payload.get("role")?.as_str()?, text)
                    }
                    StoreKind::Structured => return None,
                };
                (role == "user").then(|| RichMessage {
                    role: "user".into(),
                    blocks: vec![MessageBlock {
                        kind: "text".into(),
                        data: serde_json::json!({ "text": text }),
                    }],
                    ..Default::default()
                })
            })
            .collect();
        SessionDelta {
            new_offset: from_offset + bytes.len() as u64,
            messages,
        }
    }

    fn uses_structured_chat(&self) -> bool {
        matches!(self.kind, StoreKind::Structured)
    }

    fn read_structured_chat(&self, _: &str, session_id: &str, _: Option<&str>) -> StructuredChat {
        StructuredChat {
            cursor: Some("snapshot-cursor".into()),
            messages: self
                .structured_rows
                .lock()
                .get(session_id)
                .cloned()
                .unwrap_or_default(),
            done: None,
            observations: Vec::new(),
        }
    }
}

struct Fixture {
    service: ServiceFixture,
    tab_id: String,
    provider: LazyFirstTurnProvider,
}

impl Fixture {
    fn new(kind: StoreKind) -> Self {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("lazy-first-turn-fixture".into());
        tab.cwd = Some("/proj".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
        let service = ServiceFixture::new(mux);
        let provider = LazyFirstTurnProvider {
            root: service.temp.path().to_path_buf(),
            kind,
            structured_rows: Arc::new(Mutex::new(HashMap::new())),
        };
        service
            .service
            .agent_registry
            .register_provider(Box::new(provider.clone()));
        Self {
            service,
            tab_id,
            provider,
        }
    }

    fn start_lazy_send(&self, payload: &str) -> String {
        let evidence = self.service.service.chat_send_anchor(&self.tab_id);
        let deferred = evidence
            .deferred_first_turn
            .expect("lazy first-turn capability captures a deferred window");
        let status = self
            .service
            .service
            .chat_send_ledger
            .try_start_scoped_with_normalization(
                &self.tab_id,
                payload,
                evidence.anchor,
                evidence.live_binding,
                evidence.turn_normalization,
            )
            .unwrap();
        assert!(self
            .service
            .service
            .chat_send_ledger
            .set_deferred_first_turn_evidence(&status.send_id, deferred));
        self.service
            .service
            .chat_send_ledger
            .finish(&status.send_id, Ok(()))
            .unwrap();
        status.send_id
    }

    fn bind_session(&self, session_id: &str) {
        let mut mux = self.service.service.mux.write();
        let tab = mux.get_tab_mut(&self.tab_id).unwrap();
        tab.agent_session_id = Some(session_id.into());
        tab.reported_session_binding_nonce = Some("launch-nonce".into());
    }

    fn reconcile(&self) {
        self.service
            .service
            .reconcile_chat_send_evidence(&self.tab_id);
    }
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn lazy_codex_first_user_turn_confirms_after_binding() {
    let fixture = Fixture::new(StoreKind::Jsonl);
    let payload = "Codex first turn 🧪 exact marker";
    let send_id = fixture.start_lazy_send(payload);
    let transcript = serde_json::json!({
        "type": "response_item",
        "payload": {
            "type": "message",
            "role": "user",
            "content": [{ "type": "input_text", "text": payload }]
        }
    });
    std::fs::write(
        fixture.provider.root.join("lazy-session.jsonl"),
        format!("{transcript}\n"),
    )
    .unwrap();

    fixture.reconcile();
    assert_eq!(
        fixture
            .service
            .service
            .chat_send_ledger
            .status(&send_id)
            .unwrap()
            .state,
        ChatSendState::Unknown
    );
    fixture.bind_session("lazy-session");
    fixture.reconcile();
    assert_eq!(
        fixture
            .service
            .service
            .chat_send_ledger
            .status(&send_id)
            .unwrap()
            .state,
        ChatSendState::Sent
    );
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn lazy_structured_first_user_turn_confirms_after_binding() {
    let fixture = Fixture::new(StoreKind::Structured);
    let payload = "Structured first turn 🧪 exact marker";
    let send_id = fixture.start_lazy_send(payload);
    fixture.provider.structured_rows.lock().insert(
        "lazy-session".into(),
        vec![ChatMsg {
            id: "chat-message-1".into(),
            kind: "user".into(),
            content: payload.into(),
            ..Default::default()
        }],
    );

    fixture.bind_session("lazy-session");
    fixture.reconcile();
    assert_eq!(
        fixture
            .service
            .service
            .chat_send_ledger
            .status(&send_id)
            .unwrap()
            .state,
        ChatSendState::Sent
    );
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn lazy_structured_first_turn_ignores_foreign_and_later_matching_turns() {
    let fixture = Fixture::new(StoreKind::Structured);
    let payload = "structured first turn exact marker";
    let send_id = fixture.start_lazy_send(payload);
    fixture.provider.structured_rows.lock().insert(
        "foreign-session".into(),
        vec![ChatMsg {
            id: "foreign-user".into(),
            kind: "user".into(),
            content: payload.into(),
            ..Default::default()
        }],
    );
    fixture.provider.structured_rows.lock().insert(
        "lazy-session".into(),
        vec![
            ChatMsg {
                id: "first-user".into(),
                kind: "user".into(),
                content: "unrelated first turn".into(),
                ..Default::default()
            },
            ChatMsg {
                id: "later-user".into(),
                kind: "user".into(),
                content: payload.into(),
                ..Default::default()
            },
        ],
    );
    fixture.bind_session("lazy-session");

    fixture.reconcile();

    assert_eq!(
        fixture
            .service
            .service
            .chat_send_ledger
            .status(&send_id)
            .unwrap()
            .state,
        ChatSendState::Unknown
    );
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn lazy_first_turn_requires_the_current_reported_launch_binding() {
    let fixture = Fixture::new(StoreKind::Jsonl);
    let payload = "binding-scoped first turn";
    let send_id = fixture.start_lazy_send(payload);
    let transcript = serde_json::json!({
        "type": "response_item",
        "payload": {
            "type": "message",
            "role": "user",
            "content": [{ "type": "input_text", "text": payload }]
        }
    });
    std::fs::write(
        fixture.provider.root.join("lazy-session.jsonl"),
        format!("{transcript}\n"),
    )
    .unwrap();
    fixture.bind_session("lazy-session");
    fixture
        .service
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.tab_id)
        .unwrap()
        .reported_session_binding_nonce = Some("stale-nonce".into());

    fixture.reconcile();
    assert_eq!(
        fixture
            .service
            .service
            .chat_send_ledger
            .status(&send_id)
            .unwrap()
            .state,
        ChatSendState::Unknown
    );
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "requires the shared interactive desktop ServiceContext fixture"
)]
fn lazy_jsonl_only_accepts_the_sessions_first_user_turn() {
    let fixture = Fixture::new(StoreKind::Jsonl);
    let send_id = fixture.start_lazy_send("later matching turn");
    let prior = serde_json::json!({
        "type": "response_item",
        "payload": {
            "type": "message",
            "role": "user",
            "content": [{ "type": "input_text", "text": "other first turn" }]
        }
    });
    let later = serde_json::json!({
        "type": "response_item",
        "payload": {
            "type": "message",
            "role": "user",
            "content": [{ "type": "input_text", "text": "later matching turn" }]
        }
    });
    std::fs::write(
        fixture.provider.root.join("lazy-session.jsonl"),
        format!("{prior}\n{later}\n"),
    )
    .unwrap();
    fixture.bind_session("lazy-session");

    fixture.reconcile();
    assert_eq!(
        fixture
            .service
            .service
            .chat_send_ledger
            .status(&send_id)
            .unwrap()
            .state,
        ChatSendState::Unknown
    );
}
