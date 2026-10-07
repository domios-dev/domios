use super::*;
use crate::agent::chat_types::{MessageBlock, RichMessage, SessionDelta};
use crate::agent::provider::{AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{OrchestratorRef, Plan};
use crate::service::plan::NewTask;
use crate::service::reply_relay::runtime::RelayBinding;
use crate::service::test_fixture::ServiceFixture;
use parking_lot::Mutex;
use std::path::{Path, PathBuf};

struct ToolLessProvider {
    history: PathBuf,
    completion: std::sync::Arc<Mutex<Option<ReplyRelayEvidence>>>,
}
impl AgentProvider for ToolLessProvider {
    fn id(&self) -> &str {
        "fixture"
    }
    fn display_name(&self) -> &str {
        "fixture"
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
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
        _: &str,
        session: &str,
        after: u64,
    ) -> Option<ReplyRelayEvidence> {
        if session == "own-session" {
            self.completion
                .lock()
                .clone()
                .filter(|evidence| match evidence {
                    ReplyRelayEvidence::Answered(turn) => turn.user_record_start >= after,
                    ReplyRelayEvidence::Ended(turn) => turn.user_record_start >= after,
                    ReplyRelayEvidence::Unavailable(_) => true,
                })
        } else {
            None
        }
    }
    fn session_file_path(&self, _: &str, session: &str) -> Option<PathBuf> {
        (session == "own-session").then(|| self.history.clone())
    }
    fn session_jsonl_path(&self, cwd: &str, session: &str) -> Option<PathBuf> {
        self.session_file_path(cwd, session)
    }
    fn parse_session_delta(&self, path: &Path, offset: u64) -> SessionDelta {
        let bytes = std::fs::read(path).unwrap();
        let messages = bytes[offset as usize..]
            .split(|byte| *byte == b'\n')
            .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
            .map(|value| RichMessage {
                role: "user".into(),
                blocks: vec![MessageBlock {
                    kind: "text".into(),
                    data: serde_json::json!({"text": value["text"].as_str().unwrap()}),
                }],
                ..Default::default()
            })
            .collect();
        SessionDelta {
            messages,
            new_offset: bytes.len() as u64,
        }
    }
}

#[test]
fn exact_first_user_ack_and_final_answer_persist_a_real_worker_report() {
    let (mut mux, dispatcher) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&dispatcher).unwrap();
    let worker = mux.create_tab(&workspace).unwrap();
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&dispatcher))
        .unwrap();
    {
        let tab = mux.get_tab_mut(&worker).unwrap();
        tab.agent_type = Some("fixture".into());
        tab.agent_session_id = Some("own-session".into());
        tab.session_binding_nonce = Some("nonce".into());
        tab.reported_session_binding_nonce = Some("nonce".into());
    }
    let fixture = ServiceFixture::new(mux);
    let history = fixture.temp.path().join("own-session.jsonl");
    std::fs::write(&history, vec![b' '; 100]).unwrap();
    let completion = std::sync::Arc::new(Mutex::new(None));
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(ToolLessProvider {
            history: history.clone(),
            completion: completion.clone(),
        }));
    let mut plan = Plan::new(
        "worker fixture".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab(&dispatcher),
    );
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "opaque task".into(),
                kind: "research".into(),
                acceptance_criteria: vec![
                    "The exact own-session user turn acknowledges the assigned attempt.".into(),
                    "The final answer persists unchanged as one Worker report in review.".into(),
                ],
                test_plan: Some(
                    "Run build.ps1 test-one app exact_first_user_ack_and_final_answer_persist_a_real_worker_report --wait-build; expect one passing test proving the durable ACK and report.".into(),
                ),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    fixture
        .service
        .plan_store
        .assign_task(&plan.id, &task_id, &worker, None, 3)
        .unwrap();
    let mut pending = pending();
    pending.user_text = "<domios from=\"tab\" tab=\"dispatcher\">opaque guidance\r\nopaque executor\nopaque task [hint]</domios>".into();
    pending.turn_normalization.drop_line_breaks = true;
    let stored_user_text = pending.user_text.replace(['\r', '\n'], "");
    pending.sender_tab = dispatcher.clone();
    pending.task = fixture.service.reply_relay_task_binding(&worker);
    let binding = RelayBinding {
        provider: "fixture".into(),
        session: "own-session".into(),
        nonce: "nonce".into(),
        cwd: String::new(),
        history: history.clone(),
    };
    let task = pending.task.as_ref().unwrap();
    let ack = fixture
        .service
        .worker_ack
        .arm(crate::service::worker_ack::WorkerAckIdentity {
            plan_id: task.plan_id.clone(),
            task_id: task.task_id.clone(),
            attempt_id: task.attempt_id.clone(),
            tab_id: worker.clone(),
            startup_attempt_id: None,
            session_id: Some("own-session".into()),
            binding_nonce: Some("nonce".into()),
        })
        .receiver
        .unwrap();
    let state = Mutex::new(WorkerTurnState::default());
    fixture
        .service
        .observe_tool_less_worker_ack(&worker, &binding, &pending, &state);
    assert!(fixture
        .service
        .plan_store
        .live_task_attempts_for_tab(&worker)[0]
        .attempt
        .acknowledged_at
        .is_none());
    assert!(matches!(
        ack.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&history)
        .unwrap();
    writeln!(file, "{}", serde_json::json!({"text": stored_user_text})).unwrap();
    drop(file);
    fixture
        .service
        .observe_tool_less_worker_ack(&worker, &binding, &pending, &state);
    assert!(fixture
        .service
        .plan_store
        .live_task_attempts_for_tab(&worker)[0]
        .attempt
        .acknowledged_at
        .is_some());
    assert!(matches!(
        ack.try_recv(),
        Ok(crate::service::worker_ack::WorkerAckSignal::Acknowledged)
    ));
    let mut answered = completed();
    if let ReplyRelayEvidence::Answered(turn) = &mut answered {
        turn.user_text = stored_user_text.clone();
    }
    *completion.lock() = Some(answered);
    let visible = fixture
        .service
        .read_tool_less_session_turns(&worker, 10, 0, None, None)
        .unwrap();
    let visible = serde_json::to_value(visible).unwrap();
    assert_eq!(visible["session_id"], "own-session");
    assert_eq!(visible["turns"].as_array().unwrap().len(), 1);
    assert_eq!(visible["turns"][0]["outcome"], "answered");
    assert_eq!(visible["turns"][0]["user_text"], stored_user_text);

    let own_evidence = fixture
        .service
        .agent_registry
        .with_provider("fixture", |provider| {
            provider.read_reply_relay_turn("", "own-session", 100)
        })
        .flatten();
    let report = state
        .lock()
        .observe_completion(&pending, binding.source(), own_evidence.as_ref())
        .unwrap();
    fixture
        .service
        .publish_tool_less_worker_report(&worker, &binding, &pending, report);
    let reports = fixture.service.report_store.lock().get(&dispatcher, false);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].from_tab_id, worker);
    assert_eq!(reports[0].message, "opaque result");
    assert_eq!(reports[0].status, Some(ReportStatus::InReview));
    let task = fixture.service.plan_store.show_task(&task_id).unwrap().task;
    assert_eq!(task.status, crate::plan::TaskStatus::InReview);
    assert!(task
        .milestones
        .iter()
        .any(|stride| stride.kind == "worker_report"));
    assert!(state
        .lock()
        .observe_completion(&pending, binding.source(), Some(&completed()))
        .is_none());
}
