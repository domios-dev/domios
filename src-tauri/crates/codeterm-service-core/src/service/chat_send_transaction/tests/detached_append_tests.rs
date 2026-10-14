use super::*;
use crate::agent::chat_types::{MessageBlock, RichMessage, SessionDelta};
use crate::agent::provider::AgentProvider;
use crate::agent::registry::AgentRegistry;
use crate::service::chat_send_evidence::{
    session_append::ensure_append_stream, CapturedSendEvidence,
};
use crate::service::tab_chat::tailer::SourceRange;
use crate::service::tab_chat::{ChatRegistry, DeltaSink, TailConfig};
use std::path::PathBuf;
use std::sync::mpsc;

const SESSION: &str = "9ad1ace2-d3f0-47e9-a200-3159420c2d10";
const TAB: &str = "bab97472";
const NONCE: &str = "8fa4fb68-9281-4358-9038-309226f8af67";
const PAYLOAD: &str = "Reply with exactly this token and nothing else: DETACH-LD1791877676667";

struct EvidenceSink {
    ledger: Arc<ChatSendLedger>,
    confirmed: mpsc::Sender<ChatSendStatus>,
    observed: mpsc::Sender<()>,
}
impl DeltaSink for EvidenceSink {
    fn emit(&self, _: &str, _: Vec<RichMessage>, _: u64, _: u64) {
        panic!("append evidence must carry source provenance");
    }
    fn emit_sourced(&self, tab: &str, messages: Vec<RichMessage>, range: SourceRange) {
        let mut saw_own_user = false;
        for rich in messages {
            if rich.provider_session_id.as_deref() != Some(SESSION) || rich.role != "user" {
                continue;
            }
            saw_own_user = true;
            for message in crate::service::tab_chat::rich_to_chat_msgs_live(&rich, 0) {
                let position = TurnPosition {
                    record_start: rich.record_start,
                    record_row: rich.record_row,
                };
                let provenance = LiveProvenance {
                    source_session_id: SESSION.into(),
                    source_binding_nonce: Some(NONCE.into()),
                    range: range.clone(),
                    positions: vec![position],
                };
                self.ledger
                    .advance_live_evidence(tab, SESSION, Some(NONCE), &provenance);
                for status in self.ledger.confirm_owned_live_turns_with(
                    tab,
                    (SESSION, Some(NONCE)),
                    &message,
                    position,
                    &provenance,
                    |authority, session, nonce| {
                        authority.tab_id == TAB
                            && authority.session_id.as_deref() == Some(session)
                            && authority.binding_nonce == nonce
                    },
                ) {
                    self.confirmed.send(status).unwrap();
                }
            }
        }
        if saw_own_user {
            self.observed.send(()).unwrap();
        }
    }
}

fn detached_append_state(existing: bool, wrong_witness: bool, content: &str) -> ChatSendState {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(format!("{SESSION}.jsonl"));
    let chat = ChatRegistry::default();
    let row = |session: &str, content: &str| {
        serde_json::json!({
            "type":"user","message":{"role":"user","content":content},
            "uuid":"e4fbd52f-aa99-4437-8e0d-bad8f8345771","sessionId":session
        })
    };
    if existing {
        std::fs::write(&path, format!("{}\n", row(SESSION, PAYLOAD))).unwrap();
    }
    let source = chat.jsonl_source(TAB);
    let (anchor, witness, start_offset) = match source.capture(SESSION, &path) {
        crate::service::tab_chat::registry::SourceCapture::Absent { witness } => (
            None,
            Some(if wrong_witness {
                Arc::<str>::from("foreign-witness")
            } else {
                witness
            }),
            0,
        ),
        crate::service::tab_chat::registry::SourceCapture::Present { len } => (
            Some(SendEvidenceAnchor {
                provider_id: "windowed".into(),
                cwd: root.path().to_string_lossy().into(),
                session_id: SESSION.into(),
                window: EvidenceWindow::Jsonl {
                    transcript_offset: len,
                },
                turn_normalization: Default::default(),
            }),
            None,
            len,
        ),
        _ => panic!("fixture source must be readable"),
    };
    let ledger = Arc::new(ChatSendLedger::default());
    let record: UserSendRecord = serde_json::from_value(serde_json::json!({
        "send_id":"efcb7960-ab52-462c-82e6-1e68ce6c5e43", "client_token":null,
        "acceptance_sequence":1,"ledger_sequence":1,
        "authority":{"tab_id":TAB,"pty_id":"pty-own","binding_nonce":NONCE,
            "pty_session_generation":1,"provider_id":"windowed","session_id":SESSION,
            "observed_session_id":SESSION,"prelaunch":false},
        "owner_epoch":"epoch","reservation":1,"held_before_readiness":false,"state":"queued",
        "handoff_session":null,"typed_follow_up":null,"original_digest":"original",
        "original_bytes":PAYLOAD.len(),"prepared_digest":"prepared","prepared_bytes":PAYLOAD.len(),
        "body_digest":null,"body_path":null,"body_bytes":0
    }))
    .unwrap();
    let send = ledger.admit_user(&record);
    assert!(ledger.begin_user_handoff(
        &record,
        PAYLOAD,
        CapturedSendEvidence {
            anchor,
            absence_witness: witness,
            live_binding: Some(SendLiveBinding {
                session_id: Some(SESSION.into()),
                binding_nonce: Some(NONCE.into())
            }),
            ..Default::default()
        }
    ));
    ledger.mark_user_written(&send.send_id).unwrap();
    assert_eq!(
        ledger.finish(&send.send_id, Ok(())).unwrap().state,
        ChatSendState::Unknown
    );
    let registry = Arc::new(AgentRegistry::new());
    registry.register_provider(Box::new(ArtifactProvider {
        root: root.path().into(),
    }));
    let (tx, rx) = mpsc::channel();
    let (observed, appended) = mpsc::channel();
    assert!(ensure_append_stream(
        TailConfig {
            tab_id: TAB.into(),
            provider_id: "windowed".into(),
            cwd: root.path().to_string_lossy().into(),
            session_id: SESSION.into(),
            start_offset,
        },
        Some(NONCE),
        registry,
        &chat,
        Arc::new(EvidenceSink {
            ledger: ledger.clone(),
            confirmed: tx,
            observed,
        })
    ));
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(file, "{}", row("foreign-session", PAYLOAD)).unwrap();
    writeln!(file, "{}", row(SESSION, content)).unwrap();
    file.flush().unwrap();
    appended
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    chat.shutdown_tab(TAB);
    let state = ledger.status(&send.send_id).unwrap().state;
    if state == ChatSendState::Sent {
        let confirmed = rx.try_recv().unwrap();
        assert_eq!(confirmed.send_id, send.send_id);
        assert_eq!(confirmed.state, ChatSendState::Sent);
        assert_eq!(
            ledger
                .finish(
                    &send.send_id,
                    Err("submit_unconfirmed: late transport".into())
                )
                .unwrap()
                .state,
            ChatSendState::Sent
        );
    }
    assert!(rx.try_recv().is_err());
    state
}

#[test]
fn detached_first_materialization_confirms_owned_send_without_chat_subscription() {
    assert_eq!(
        detached_append_state(false, false, PAYLOAD),
        ChatSendState::Sent
    );
}

#[test]
fn detached_existing_store_does_not_confirm_from_pre_send_history_or_partial_payload() {
    assert_eq!(
        detached_append_state(true, false, "DETACH-LD1791877676667"),
        ChatSendState::Unknown
    );
    assert_eq!(
        detached_append_state(true, false, PAYLOAD),
        ChatSendState::Sent
    );
}

#[test]
fn detached_first_materialization_requires_the_captured_absence_witness_and_whole_payload() {
    assert_eq!(
        detached_append_state(false, true, PAYLOAD),
        ChatSendState::Unknown
    );
    assert_eq!(
        detached_append_state(false, false, "DETACH-LD1791877676667"),
        ChatSendState::Unknown
    );
}

struct ArtifactProvider {
    root: PathBuf,
}

impl AgentProvider for ArtifactProvider {
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
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(path).unwrap();
        file.seek(SeekFrom::Start(from_offset)).unwrap();
        let mut window = Vec::new();
        file.read_to_end(&mut window).unwrap();
        let mut record_start = from_offset;
        let messages = window
            .split_inclusive(|byte| *byte == b'\n')
            .filter_map(|line| {
                let start = record_start;
                record_start += line.len() as u64;
                let value: serde_json::Value = serde_json::from_slice(line).ok()?;
                let message = value.get("message")?;
                let role = message.get("role")?.as_str()?.to_string();
                let content = message.get("content")?.as_str()?.to_string();
                Some(RichMessage {
                    role,
                    uuid: value.get("uuid")?.as_str().map(str::to_string),
                    provider_session_id: value.get("sessionId")?.as_str().map(str::to_string),
                    record_start: Some(start),
                    blocks: vec![MessageBlock {
                        kind: "text".into(),
                        data: serde_json::json!({"text":content}),
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

#[test]
fn session_append_assistant_echo_cannot_confirm_delivery() {
    let ledger = ChatSendLedger::default();
    let send = ledger
        .try_start_scoped(
            TAB,
            PAYLOAD,
            None,
            Some(SendLiveBinding {
                session_id: Some(SESSION.into()),
                binding_nonce: Some(NONCE.into()),
            }),
        )
        .unwrap();
    ledger.finish(&send.send_id, Ok(())).unwrap();
    let mut message = crate::agent::chat_types::ChatMsg {
        id: "assistant-echo".into(),
        kind: "assistant".into(),
        content: PAYLOAD.into(),
        ..Default::default()
    };
    assert!(delivered_turn_confirmations(
        &ledger,
        TAB,
        SESSION,
        Some(NONCE),
        &[message.clone()],
        None
    )
    .is_empty());
    assert_eq!(
        ledger.status(&send.send_id).unwrap().state,
        ChatSendState::Unknown
    );
    message.kind = "user".into();
    message.id = "own-user".into();
    let confirmed =
        delivered_turn_confirmations(&ledger, TAB, SESSION, Some(NONCE), &[message], None);
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed[0].send_id, send.send_id);
    assert_eq!(confirmed[0].state, ChatSendState::Sent);
}
